use std::borrow::Cow;

use spinoff::{spinners, Color, Spinner, Streams};

pub fn new_spinner(message: impl Into<Cow<'static, str>>, stream: Streams) -> Spinner {
    Spinner::new_with_stream(spinners::Dots, message, Color::Cyan, stream)
}

pub fn request_spinner() -> Spinner {
    new_spinner("Request in progress...", Streams::Stderr)
}
