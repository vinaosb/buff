async fn fetch_data() -> i64 {
    return 42;
}
async fn pipeline() -> i64 {
    return fetch_data().await;
}
#[tokio::main]
async fn main() {
    let result = pipeline().await;
    println!("{}", result);
    let task = tokio::spawn(async move { fetch_data().await });
    let answer = task.await;
    println!("{}", answer);
}
