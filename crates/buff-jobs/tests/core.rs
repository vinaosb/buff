use buff_jobs::{
    Backoff, Job, JobId, JobStatus, JobsError, Priority, Queue, QueueStats, Scheduler, Worker,
    WorkerStats,
};
use std::time::Duration;

fn make_job(payload: &str) -> Job {
    Job::new(payload).expect("test job")
}

#[test]
fn job_new_rejects_empty_payload() {
    assert!(matches!(Job::new(""), Err(JobsError::InvalidJob(_))));
}

#[test]
fn job_new_generates_uuid_id() {
    let job = make_job("hello");
    assert!(!job.id().as_str().is_empty());
    assert_eq!(job.payload(), "hello");
    assert_eq!(job.priority(), Priority::Normal);
    assert_eq!(job.max_retries(), 3);
    assert_eq!(job.status(), JobStatus::Pending);
}

#[test]
fn job_builder_sets_fields() {
    let job = make_job("x")
        .with_priority(Priority::Critical)
        .with_max_retries(7)
        .with_backoff(Backoff::exponential(
            Duration::from_millis(200),
            Duration::from_secs(30),
        ));
    assert_eq!(job.priority(), Priority::Critical);
    assert_eq!(job.max_retries(), 7);
    assert!(matches!(job.backoff(), Backoff::Exponential { .. }));
}

#[test]
fn job_next_retry_delay_returns_none_when_exhausted() {
    let job = make_job("done").with_max_retries(0);
    assert!(job.next_retry_delay().expect("ok").is_none());
}

#[test]
fn queue_enqueue_then_dequeue_preserves_fifo() {
    let q = Queue::memory();
    q.enqueue(make_job("a")).expect("enqueue");
    q.enqueue(make_job("b")).expect("enqueue");
    q.enqueue(make_job("c")).expect("enqueue");
    assert_eq!(q.len(), 3);
    assert_eq!(q.dequeue().expect("dequeue").expect("job").payload(), "a");
    assert_eq!(q.dequeue().expect("dequeue").expect("job").payload(), "b");
    assert_eq!(q.dequeue().expect("dequeue").expect("job").payload(), "c");
    assert!(q.dequeue().expect("dequeue").is_none());
}

#[test]
fn queue_priority_deques_critical_first() {
    let q = Queue::memory();
    q.enqueue(make_job("low").with_priority(Priority::Low))
        .expect("enqueue");
    q.enqueue(make_job("critical").with_priority(Priority::Critical))
        .expect("enqueue");
    q.enqueue(make_job("normal").with_priority(Priority::Normal))
        .expect("enqueue");
    assert_eq!(
        q.dequeue().expect("dequeue").expect("job").payload(),
        "critical"
    );
    assert_eq!(
        q.dequeue().expect("dequeue").expect("job").payload(),
        "normal"
    );
    assert_eq!(q.dequeue().expect("dequeue").expect("job").payload(), "low");
}

#[test]
fn queue_equal_priority_preserves_fifo() {
    let q = Queue::memory();
    q.enqueue(make_job("first").with_priority(Priority::High))
        .expect("enqueue");
    q.enqueue(make_job("second").with_priority(Priority::High))
        .expect("enqueue");
    assert_eq!(
        q.dequeue().expect("dequeue").expect("job").payload(),
        "first"
    );
    assert_eq!(
        q.dequeue().expect("dequeue").expect("job").payload(),
        "second"
    );
}

#[test]
fn queue_stats_track_transitions() {
    let q = Queue::memory();
    q.enqueue(make_job("a")).expect("enqueue");
    q.enqueue(make_job("b")).expect("enqueue");
    let _ = q.dequeue().expect("dequeue");
    let stats = q.stats();
    assert_eq!(stats.pending, 1);
    assert_eq!(stats.in_flight, 1);
}

#[test]
fn backoff_fixed_is_constant() {
    let b = Backoff::fixed(Duration::from_millis(500));
    assert_eq!(b.delay(1, 5).unwrap(), Duration::from_millis(500));
    assert_eq!(b.delay(5, 5).unwrap(), Duration::from_millis(500));
}

#[test]
fn backoff_exponential_doubles_and_caps() {
    let b = Backoff::exponential(Duration::from_millis(100), Duration::from_secs(8));
    assert_eq!(b.delay(1, 10).unwrap(), Duration::from_millis(100));
    assert_eq!(b.delay(2, 10).unwrap(), Duration::from_millis(200));
    assert_eq!(b.delay(3, 10).unwrap(), Duration::from_millis(400));
    assert_eq!(b.delay(4, 10).unwrap(), Duration::from_millis(800));
    assert_eq!(b.delay(20, 20).unwrap(), Duration::from_secs(8));
}

#[test]
fn backoff_rejects_attempt_zero_or_over_max() {
    let b = Backoff::fixed(Duration::from_secs(1));
    assert!(b.delay(0, 5).is_err());
    assert!(b.delay(6, 5).is_err());
}

#[test]
fn worker_drains_and_succeeds() {
    let q = Queue::memory();
    q.enqueue(make_job("a")).expect("enqueue");
    q.enqueue(make_job("b")).expect("enqueue");
    let w = Worker::new(q.clone());
    let stats = w.run(|_| Ok(())).expect("worker run");
    assert_eq!(stats.processed, 2);
    assert_eq!(stats.succeeded, 2);
    assert_eq!(stats.failed, 0);
    assert!(q.is_empty());
    assert_eq!(q.stats().completed, 2);
}

#[test]
fn worker_retries_until_success() {
    let q = Queue::memory();
    q.enqueue(
        make_job("flaky")
            .with_max_retries(3)
            .with_backoff(Backoff::fixed(Duration::ZERO)),
    )
    .expect("enqueue");
    let w = Worker::new(q.clone());
    let mut attempts = 0u32;
    let stats = w
        .run(|_| {
            attempts += 1;
            if attempts < 2 {
                Err("transient".to_string())
            } else {
                Ok(())
            }
        })
        .expect("worker run");
    assert_eq!(stats.succeeded, 1);
    assert!(stats.failed >= 1);
    assert_eq!(q.stats().completed, 1);
    assert!(q.dead_letter().is_empty());
}

#[test]
fn worker_routes_to_dead_letter_when_budget_exhausted() {
    let q = Queue::memory();
    q.enqueue(
        make_job("doomed")
            .with_max_retries(2)
            .with_backoff(Backoff::fixed(Duration::ZERO)),
    )
    .expect("enqueue");
    let w = Worker::new(q.clone());
    let stats = w.run(|_| Err("permanent".to_string())).expect("worker run");
    assert_eq!(stats.processed, 3);
    assert_eq!(stats.succeeded, 0);
    assert_eq!(stats.dead_lettered, 1);
    assert_eq!(q.dead_letter().len(), 1);
    assert_eq!(q.dead_letter()[0].payload(), "doomed");
    assert_eq!(q.dead_letter()[0].status(), JobStatus::DeadLetter);
}

#[test]
fn worker_priority_ordering_honored() {
    let q = Queue::memory();
    let mut observed: Vec<String> = Vec::new();
    q.enqueue(make_job("low").with_priority(Priority::Low))
        .expect("enqueue");
    q.enqueue(make_job("high").with_priority(Priority::High))
        .expect("enqueue");
    let w = Worker::new(q.clone());
    w.run(|job| {
        observed.push(job.payload().to_string());
        Ok(())
    })
    .expect("worker run");
    assert_eq!(observed, vec!["high".to_string(), "low".to_string()]);
}

#[test]
fn scheduler_cron_valid_registers() {
    let rt = tokio::runtime::Runtime::new().expect("rt");
    rt.block_on(async {
        let sched = Scheduler::new();
        let id = sched
            .cron("0 0 * * * *", make_job("hourly"))
            .await
            .expect("cron");
        assert_eq!(sched.pending_count().await, 1);
        let removed = sched.remove(id).await;
        assert!(removed);
        assert_eq!(sched.pending_count().await, 0);
    });
}

#[test]
fn scheduler_cron_invalid_returns_error() {
    let rt = tokio::runtime::Runtime::new().expect("rt");
    rt.block_on(async {
        let sched = Scheduler::new();
        let result = sched.cron("not-valid", make_job("x")).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            JobsError::InvalidCron { .. } => {}
            other => panic!("expected InvalidCron, got {:?}", other),
        }
    });
}

#[test]
fn scheduler_interval_registers() {
    let rt = tokio::runtime::Runtime::new().expect("rt");
    rt.block_on(async {
        let sched = Scheduler::new();
        let _id = sched
            .interval(Duration::from_secs(60), make_job("minutely"))
            .await
            .expect("interval");
        assert_eq!(sched.pending_count().await, 1);
    });
}

#[test]
fn scheduler_remove_nonexistent_returns_false() {
    let rt = tokio::runtime::Runtime::new().expect("rt");
    rt.block_on(async {
        let sched = Scheduler::new();
        let fake_id = buff_jobs::JobId("nonexistent".to_string());
        let removed = sched.remove(fake_id).await;
        assert!(!removed);
    });
}

#[test]
fn snapshot_priority_display() {
    insta::assert_snapshot!(
        "priority_display",
        format!(
            "{}|{}|{}|{}",
            Priority::Low,
            Priority::Normal,
            Priority::High,
            Priority::Critical
        )
    );
}

#[test]
fn snapshot_job_status_display() {
    insta::assert_snapshot!(
        "job_status_display",
        format!(
            "{}|{}|{}|{}|{}",
            JobStatus::Pending,
            JobStatus::InProgress,
            JobStatus::Completed,
            JobStatus::Failed,
            JobStatus::DeadLetter
        )
    );
}

#[test]
fn snapshot_backoff_display() {
    insta::assert_snapshot!(
        "backoff_display",
        format!(
            "{}|{}|{}",
            Backoff::fixed(Duration::from_secs(1)),
            Backoff::linear(Duration::from_secs(1)),
            Backoff::exponential(Duration::from_secs(1), Duration::from_secs(60))
        )
    );
}

#[test]
fn snapshot_job_display() {
    let job = make_job("payload-x")
        .with_priority(Priority::High)
        .with_max_retries(5);
    insta::assert_snapshot!("job_display", format!("{job}"));
}

#[test]
fn snapshot_worker_stats_display() {
    let stats = WorkerStats {
        processed: 100,
        succeeded: 95,
        failed: 5,
        retried: 4,
        dead_lettered: 1,
    };
    insta::assert_snapshot!("worker_stats_display", format!("{stats}"));
}
