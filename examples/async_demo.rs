async fn fetch_value() -> i64 {
    return 42;
}
async fn pipeline() -> i64 {
    return fetch_value().await;
}
#[tokio::main]
async fn main() {
    let value = pipeline().await;
    println!("{}", value);
    let task = tokio::spawn(async move { fetch_value().await });
    let answer = task.await;
    println!("{}", answer);
}
