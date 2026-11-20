//! Command-line parsing used by open-with, drag & drop relaunch and the shell.
//!
//! Pure and platform-independent so it can be unit-tested; the app converts
//! `OsString`s to strings and passes them here.

use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CliError {
    #[error("missing value for option {0}")]
    MissingValue(&'static str),
    #[error("invalid value for {0}: {1}")]
    InvalidValue(&'static str, String),
    #[error("unknown option: {0}")]
    UnknownOption(String),
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Cli {
    /// Files or MRLs to play, in order.
    pub media: Vec<String>,
    pub runtime: Option<PathBuf>,
    pub size: Option<(u32, u32)>,
    pub start_time: Option<Duration>,
    /// External subtitle files to load (SRT/WebVTT/ASS/SSA).
    pub subtitle_files: Vec<String>,
    /// Per-item cap used by the headless engine harness and smoke tests.
    pub max_seconds: Option<u64>,
    pub rtl: bool,
    pub fullscreen: bool,
    pub no_video: bool,
}

impl Cli {
    /// Parses arguments excluding the program name. `--` stops option parsing.
    pub fn parse(args: &[String]) -> Result<Self, CliError> {
        let mut cli = Self::default();
        let mut i = 0;
        let mut options = true;
        while i < args.len() {
            let arg = &args[i];
            if options && arg == "--" {
                options = false;
            } else if options && let Some(name) = arg.strip_prefix("--") {
                match name {
                    "rtl" => cli.rtl = true,
                    "fullscreen" => cli.fullscreen = true,
                    "no-video" => cli.no_video = true,
                    "runtime" => {
                        cli.runtime = Some(PathBuf::from(Self::value(args, &mut i, "--runtime")?));
                    }
                    "size" => {
                        let raw = Self::value(args, &mut i, "--size")?;
                        cli.size = Some(Self::parse_size(raw)?);
                    }
                    "sub-file" => {
                        let raw = Self::value(args, &mut i, "--sub-file")?;
                        cli.subtitle_files.push(raw.to_owned());
                    }
                    "max-seconds" => {
                        let raw = Self::value(args, &mut i, "--max-seconds")?;
                        let secs: u64 = raw
                            .parse()
                            .map_err(|_| CliError::InvalidValue("--max-seconds", raw.to_owned()))?;
                        cli.max_seconds = Some(secs);
                    }
                    "start-time" => {
                        let raw = Self::value(args, &mut i, "--start-time")?;
                        let secs: f64 = raw
                            .parse()
                            .map_err(|_| CliError::InvalidValue("--start-time", raw.to_owned()))?;
                        if !secs.is_finite() || secs < 0.0 {
                            return Err(CliError::InvalidValue("--start-time", raw.to_owned()));
                        }
                        cli.start_time = Some(Duration::from_secs_f64(secs));
                    }
                    _ => return Err(CliError::UnknownOption(arg.clone())),
                }
            } else {
                cli.media.push(arg.clone());
            }
            i += 1;
        }
        Ok(cli)
    }

    fn value<'a>(
        args: &'a [String],
        i: &mut usize,
        name: &'static str,
    ) -> Result<&'a str, CliError> {
        *i += 1;
        args.get(*i)
            .map(String::as_str)
            .ok_or(CliError::MissingValue(name))
    }

    fn parse_size(raw: &str) -> Result<(u32, u32), CliError> {
        let (w, h) = raw
            .split_once(['x', 'X'])
            .ok_or_else(|| CliError::InvalidValue("--size", raw.to_owned()))?;
        let w: u32 = w
            .parse()
            .map_err(|_| CliError::InvalidValue("--size", raw.to_owned()))?;
        let h: u32 = h
            .parse()
            .map_err(|_| CliError::InvalidValue("--size", raw.to_owned()))?;
        if w == 0 || h == 0 {
            return Err(CliError::InvalidValue("--size", raw.to_owned()));
        }
        Ok((w, h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn parses_media_and_flags() {
        let cli = Cli::parse(&args(&["a.mp4", "--rtl", "b.mkv", "--fullscreen"])).unwrap();
        assert_eq!(cli.media, vec!["a.mp4", "b.mkv"]);
        assert!(cli.rtl);
        assert!(cli.fullscreen);
    }

    #[test]
    fn parses_options_with_values() {
        let cli = Cli::parse(&args(&[
            "--runtime",
            "C:/vlc",
            "--size",
            "3840x2160",
            "--start-time",
            "12.5",
            "--max-seconds",
            "3",
            "--sub-file",
            "subs.srt",
            "a.mp4",
        ]))
        .unwrap();
        assert_eq!(cli.runtime, Some(PathBuf::from("C:/vlc")));
        assert_eq!(cli.size, Some((3840, 2160)));
        assert_eq!(cli.start_time, Some(Duration::from_millis(12_500)));
        assert_eq!(cli.max_seconds, Some(3));
        assert_eq!(cli.subtitle_files, vec!["subs.srt"]);
    }

    #[test]
    fn double_dash_forces_media_names() {
        let cli = Cli::parse(&args(&["--", "--not-an-option.mp4"])).unwrap();
        assert_eq!(cli.media, vec!["--not-an-option.mp4"]);
    }

    #[test]
    fn rejects_bad_options() {
        assert_eq!(
            Cli::parse(&args(&["--bogus"])).unwrap_err(),
            CliError::UnknownOption("--bogus".into())
        );
        assert_eq!(
            Cli::parse(&args(&["--size"])).unwrap_err(),
            CliError::MissingValue("--size")
        );
        assert!(matches!(
            Cli::parse(&args(&["--size", "0x10"])),
            Err(CliError::InvalidValue(_, _))
        ));
        assert!(matches!(
            Cli::parse(&args(&["--start-time", "-1"])),
            Err(CliError::InvalidValue(_, _))
        ));
    }
}
