// T34 example: RBAC policy — build, then enforce.

use buff_auth::Rbac;

fn main() {
    let mut policy = Rbac::new();
    policy.add("admin", "*", "read").expect("admin rule 1");
    policy
        .add("admin", "users", "delete")
        .expect("admin rule 2");
    policy.add("user", "posts", "read").expect("user rule 1");
    policy.add("*", "health", "read").expect("anon rule 1");

    println!("policy size: {}", policy.len());

    let roles = vec!["admin".to_string()];
    let can_delete = policy.enforce(&roles, "users", "delete");
    let can_read_anything = policy.enforce(&roles, "anything", "read");
    println!("admin can delete users: {can_delete}");
    println!("admin can read anything: {can_read_anything}");

    let anon_roles: Vec<String> = vec![];
    let anon_can_read_health = policy.enforce(&anon_roles, "health", "read");
    println!("anon can read health: {anon_can_read_health}");
}
