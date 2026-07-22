// T35 example: cron + interval scheduling.
//
// Demonstrates the Scheduler API. Registers a cron-expression
// schedule (top of every hour), a fixed-interval schedule (every
// 60 seconds), and a weekday-morning schedule. Prints each
// schedule's next fire time relative to "now".

use buff_jobs::{Job, Scheduler};
use chrono::Utc;
use std::time::Duration;

fn main() {
    let scheduler = Scheduler::new()
        .cron("0 0 * * * *", Job::new("hourly-report").unwrap())
        .expect("cron parses")
        .interval(Duration::from_secs(60), Job::new("health-check").unwrap())
        .cron("0 0 9 * * Mon-Fri *", Job::new("weekday-morning").unwrap())
        .expect("weekday cron parses");

    println!("registered {} schedules:", scheduler.len());
    for s in scheduler.schedules() {
        println!("  - {s}");
    }

    let now = Utc::now();
    match scheduler.next_due(now).expect("next_due") {
        Some(s) => {
            let next = s.next_fire(now).expect("next_fire").expect("reachable");
            let delta = next.signed_duration_since(now).num_seconds();
            println!(
                "next due: {} (fires in {}s = {})",
                s.job().payload(),
                delta,
                next
            );
        }
        None => println!("no schedules due"),
    }
}
