//! Executable da tool web.fetch: le a requisicao da entrada padrao e
//! responde em linha unica.

mod extract;
mod fetch;
mod protocol;

use std::io::Read;

fn respond(target: &str, result: Result<fetch::Fetched, String>) {
    match result {
        Ok(fetched) => println!(
            "{}",
            protocol::response(
                target,
                "success",
                "",
                Some(&fetched.url),
                Some(&fetched.content_type),
                fetched.title.as_deref(),
                Some(&fetched.content),
            )
        ),
        Err(error) => println!(
            "{}",
            protocol::response(target, "failed", &error, None, None, None, None)
        ),
    }
}

fn main() {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        println!(
            "{}",
            protocol::response(
                "",
                "failed",
                "failed to read request",
                None,
                None,
                None,
                None
            )
        );
        return;
    }
    match protocol::parse_request(&input) {
        Ok(request) => respond(&request.target, fetch::fetch(&request.url)),
        Err(error) => println!(
            "{}",
            protocol::response("", "failed", &error, None, None, None, None)
        ),
    }
}
