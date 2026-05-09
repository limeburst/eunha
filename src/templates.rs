use minijinja::{Environment, Value};
use once_cell::sync::Lazy;

static ENV: Lazy<Environment<'static>> = Lazy::new(|| {
    let mut env = Environment::new();
    env.add_template_owned(
        "authorize.html",
        include_str!("templates/authorize.html").to_string(),
    )
    .expect("authorize.html template is invalid");
    env.add_template_owned(
        "unknown_host.html",
        include_str!("templates/unknown_host.html").to_string(),
    )
    .expect("unknown_host.html template is invalid");
    env.add_template_owned(
        "signup.html",
        include_str!("templates/signup.html").to_string(),
    )
    .expect("signup.html template is invalid");
    env
});

pub fn render(name: &str, ctx: Value) -> String {
    ENV.get_template(name)
        .and_then(|t| t.render(ctx))
        .unwrap_or_else(|e| format!("Template error: {e}"))
}
