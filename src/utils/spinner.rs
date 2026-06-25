use std::borrow::Cow;

use spinoff::{Color, Spinner, Streams};

fn spinner_frames() -> spinoff::spinners::SpinnerFrames {
    #[cfg(windows)]
    {
        spinoff::spinners::Line.into()
    }

    #[cfg(not(windows))]
    {
        spinoff::spinners::Dots.into()
    }
}

pub fn new_spinner(message: impl Into<Cow<'static, str>>, stream: Streams) -> Spinner {
    Spinner::new_with_stream(spinner_frames(), message, Color::Cyan, stream)
}

pub fn request_spinner() -> Spinner {
    new_spinner("Request in progress...", Streams::Stderr)
}

#[cfg(test)]
mod tests {
    use super::spinner_frames;

    #[cfg(windows)]
    #[test]
    fn uses_ascii_spinner_frames_on_windows() {
        let frames = spinner_frames();
        assert_eq!(frames.frames, vec!["-", "\\", "|", "/"]);
        assert_eq!(frames.interval, 130);
    }

    #[cfg(not(windows))]
    #[test]
    fn uses_unicode_spinner_frames_on_non_windows() {
        let frames = spinner_frames();
        assert_eq!(
            frames.frames,
            vec!["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
        );
        assert_eq!(frames.interval, 80);
    }
}
