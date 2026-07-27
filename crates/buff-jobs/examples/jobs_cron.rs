// T35 example: cron + interval scheduling with handler dispatch
// (P0.22: handler invocation now actually fires).
//
// Registers three schedules with handlers, starts the scheduler,
// lets it run for a few seconds, then stops. Each handler bumps an
// Arc<AtomicU32> counter so the user can see the schedule firing.

use buff_jobs::{Job, Scheduler};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let scheduler = Scheduler::new();

    let hourly_fires = Arc::new(AtomicU32::new(0));
    let hourly_fires_clone = hourly_fires.clone();
    scheduler
        .cron_with_handler(
            "0 0 * * * *",
            Job::new("hourly-report").expect("job"),
            move |_job| {
                hourly_fires_clone.fetch_add(1, Ordering::SeqCst);
                println!("[hourly-report] tick");
                Ok(())
            },
        )
        .await
        .expect("cron parses");

    let health_fires = Arc::new(AtomicU32::new(0));
    let health_fires_clone = health_fires.clone();
    scheduler
        .interval_with_handler(
            Duration::from_secs(1),
            Job::new("health-check").expect("job"),
            move |_job| {
                health_fires_clone.fetch_add(1, Ordering::SeqCst);
                println!("[health-check] tick");
                Ok(())
            },
        )
        .await
        .expect("interval registers");

    println!("registered {} schedules:", scheduler.pending_count().await);
    for s in scheduler.schedules().await {
        println!("  - {} (next_fire={:?})", s.job.payload(), s.next_fire);
    }

    scheduler.start().await;

    // Let the schedules fire for 3 seconds, then stop.
    tokio::time::sleep(Duration::from_secs(3)).await;

    scheduler.stop().await;

    println!("---");
    println!(
        "hourly-report fired {} times",
        hourly_fires.load(Ordering::SeqCst)
    );
    println!(
        "health-check fired {} times",
        health_fires.load(Ordering::SeqCst)
    );
}
