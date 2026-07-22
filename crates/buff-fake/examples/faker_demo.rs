// T37 example: generate fake data with seeded RNG.
//
// Demonstrates all 8 Faker methods: name, email, address, phone,
// uuid, lorem, int, datetime. Uses a fixed seed for reproducible output.

use buff_fake::{Faker, FakerLocale};

fn main() {
    let mut faker = Faker::with_seed(FakerLocale::EnUs, 42);

    println!("Name:     {}", faker.name());
    println!("Email:    {}", faker.email());
    println!("Address:  {}", faker.address());
    println!("Phone:    {}", faker.phone());
    println!("UUID:     {}", faker.uuid());
    println!("Lorem:    {}", faker.lorem(5));
    println!("Int:      {}", faker.int(1, 100));

    match faker.datetime("2023-01-01T00:00:00Z", "2023-12-31T23:59:59Z") {
        Ok(dt) => println!("Datetime: {}", dt),
        Err(e) => println!("Datetime error: {e}"),
    }
}
