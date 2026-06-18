use std::borrow::Cow;

use spinoff::{spinners, Color, Spinner, Streams};

pub fn new_spinner(message: impl Into<Cow<'static, str>>, stream: Streams) -> Spinner {
    #[cfg(windows)]
    {
        Spinner::new_with_stream(spinners::Line, message, Color::Cyan, stream)
    }

    #[cfg(not(windows))]
    {
        Spinner::new_with_stream(spinners::Dots, message, Color::Cyan, stream)
    }
}

pub fn request_spinner() -> Spinner {
    new_spinner("Request in progress...", Streams::Stderr)
}
