//! AI-003 acceptance smoke test: one real request to a hosted model.
//!
//! Run in your own terminal so the key never enters a transcript or file:
//!
//! ```text
//! export ANTHROPIC_API_KEY=...   # or OPENAI_API_KEY
//! cargo run -p wiki-ai --example hosted_smoke -- anthropic claude-haiku-5-5
//! cargo run -p wiki-ai --example hosted_smoke -- openai gpt-4o-mini
//! ```
//!
//! This sends real cloud traffic billed to that key. It prints the reply and
//! token counts, and never prints the key.

use wiki_ai::hosted_codec::{ApiKey, HostedProvider};
use wiki_ai::hosted_http::HostedChatProvider;
use wiki_ai::{
    stream_selected, CancellationToken, GenerateRequest, Message, MessageRole, OutboundPolicy,
    StreamEvent,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (provider, variable) = match args.first().map(String::as_str) {
        Some("anthropic") => (HostedProvider::Anthropic, "ANTHROPIC_API_KEY"),
        Some("openai") => (HostedProvider::OpenAi, "OPENAI_API_KEY"),
        _ => {
            eprintln!("usage: hosted_smoke <anthropic|openai> <model>");
            std::process::exit(2);
        }
    };
    let Some(model) = args.get(1) else {
        eprintln!("usage: hosted_smoke <anthropic|openai> <model>");
        std::process::exit(2);
    };
    let Ok(secret) = std::env::var(variable) else {
        eprintln!("{variable} is not set");
        std::process::exit(2);
    };
    let key = ApiKey::new(secret).unwrap_or_else(|_| {
        eprintln!("{variable} is not a valid API key value");
        std::process::exit(2);
    });
    let hosted = HostedChatProvider::new(provider, key).unwrap_or_else(|error| {
        eprintln!("hosted provider unavailable: {error:?}");
        std::process::exit(1);
    });
    let request = GenerateRequest {
        model: model.clone(),
        messages: vec![
            Message {
                role: MessageRole::System,
                content: "Answer in one short sentence.".into(),
            },
            Message {
                role: MessageRole::User,
                content: "What does an encyclopedia contain?".into(),
            },
        ],
        max_output_tokens: 64,
    };
    let mut usage = (None, None);
    print!("reply: ");
    let result = stream_selected(
        &hosted,
        &request,
        &CancellationToken::default(),
        OutboundPolicy::AllowCloud,
        &mut |event| {
            match event {
                StreamEvent::TextDelta(text) => print!("{text}"),
                StreamEvent::Completed {
                    input_tokens,
                    output_tokens,
                } => usage = (input_tokens, output_tokens),
            }
            Ok(())
        },
    );
    println!();
    match result {
        Ok(()) => println!(
            "hosted_smoke ok: provider={provider:?} model={model} input_tokens={:?} output_tokens={:?}",
            usage.0, usage.1
        ),
        Err(error) => {
            println!("hosted_smoke FAILED: provider={provider:?} model={model} error={error:?}");
            std::process::exit(1);
        }
    }
}
