use spinoff::{spinners, Color, Spinner, Streams};

pub fn request_spinner() -> Spinner {
    Spinner::new_with_stream(
        spinners::Dots,
        "Request in progress...",
        Color::White,
        Streams::Stderr,
    )
}
