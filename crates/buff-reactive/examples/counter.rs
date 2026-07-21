use buff_reactive::{batch, Computed, Effect, Signal};

fn main() {
    let count = Signal::new(0);
    let doubled: Computed<i64> = {
        let count = count.clone();
        Computed::new(move || count.get() * 2)
    };

    Effect::new({
        let doubled = doubled.clone();
        move || {
            println!("doubled = {}", doubled.get());
        }
    });

    println!("--- increment one at a time ---");
    count.set(1);
    count.set(2);
    count.set(3);

    println!("--- batch increments (single notification) ---");
    let count_for_batch = count.clone();
    batch(move || {
        count_for_batch.set(10);
        count_for_batch.set(20);
        count_for_batch.set(30);
    });

    println!("--- final doubled = {} ---", doubled.get());
}
