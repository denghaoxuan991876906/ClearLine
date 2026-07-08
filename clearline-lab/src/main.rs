use std::{env, path::PathBuf, time::Duration};

use anyhow::{anyhow, Context, Result};

#[cfg(windows)]
mod recorder;

#[cfg(not(windows))]
mod recorder {
    use anyhow::{bail, Result};

    use super::RecordOptions;

    pub fn list_devices() -> Result<()> {
        bail!("clearline-lab recording is only available on Windows")
    }

    pub fn record_device(_options: RecordOptions) -> Result<()> {
        bail!("clearline-lab recording is only available on Windows")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    List,
    Record(RecordOptions),
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordOptions {
    pub device_query: String,
    pub duration: Duration,
    pub out: PathBuf,
}

fn main() -> Result<()> {
    match parse_args(env::args()) {
        Ok(Command::List) => recorder::list_devices(),
        Ok(Command::Record(options)) => recorder::record_device(options),
        Ok(Command::Help) => {
            print_usage();
            Ok(())
        }
        Err(message) => {
            print_usage();
            Err(anyhow!(message))
        }
    }
}

fn parse_args<I, S>(args: I) -> std::result::Result<Command, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = args.into_iter().map(Into::into);
    let _program = args.next();
    let Some(command) = args.next() else {
        return Ok(Command::Help);
    };

    match command.as_str() {
        "list" => {
            reject_extra_args(args)?;
            Ok(Command::List)
        }
        "record" => parse_record_args(args),
        "help" | "--help" | "-h" => Ok(Command::Help),
        other => Err(format!("unknown command: {other}")),
    }
}

fn reject_extra_args(mut args: impl Iterator<Item = String>) -> std::result::Result<(), String> {
    match args.next() {
        Some(extra) => Err(format!("unexpected argument: {extra}")),
        None => Ok(()),
    }
}

fn parse_record_args(args: impl Iterator<Item = String>) -> std::result::Result<Command, String> {
    let mut device_query = None;
    let mut seconds = None;
    let mut out = None;
    let mut args = args.peekable();

    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--device" | "-d" => {
                device_query = Some(next_value(&mut args, &flag)?);
            }
            "--seconds" | "-s" => {
                let value = next_value(&mut args, &flag)?;
                let parsed = value
                    .parse::<u64>()
                    .map_err(|_| format!("--seconds must be a positive integer: {value}"))?;
                if parsed == 0 {
                    return Err("--seconds must be greater than zero".to_owned());
                }
                seconds = Some(parsed);
            }
            "--out" | "-o" => {
                out = Some(PathBuf::from(next_value(&mut args, &flag)?));
            }
            "--help" | "-h" => return Ok(Command::Help),
            other => return Err(format!("unexpected record argument: {other}")),
        }
    }

    let device_query = device_query
        .context("missing --device <name-or-index>")
        .map_err(|e| e.to_string())?;
    let seconds = seconds
        .context("missing --seconds <n>")
        .map_err(|e| e.to_string())?;
    let out = out
        .context("missing --out <path.wav>")
        .map_err(|e| e.to_string())?;

    Ok(Command::Record(RecordOptions {
        device_query,
        duration: Duration::from_secs(seconds),
        out,
    }))
}

fn next_value(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
) -> std::result::Result<String, String> {
    args.next()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn print_usage() {
    eprintln!(
        "ClearLine lab recorder\n\n\
Usage:\n\
  clearline-lab list\n\
  clearline-lab record --device <name-or-index> --seconds <n> --out <path.wav>\n\n\
Examples:\n\
  clearline-lab list\n\
  clearline-lab record --device \"MCHOSE\" --seconds 12 --out artifacts/lab/raw.wav\n\
  clearline-lab record --device 2 --seconds 12 --out artifacts/lab/clearline.wav"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_list_command() {
        assert_eq!(parse_args(["clearline-lab", "list"]), Ok(Command::List));
    }

    #[test]
    fn parses_record_command_with_long_flags() {
        let command = parse_args([
            "clearline-lab",
            "record",
            "--device",
            "mic",
            "--seconds",
            "2",
            "--out",
            "out.wav",
        ]);

        assert_eq!(
            command,
            Ok(Command::Record(RecordOptions {
                device_query: "mic".to_owned(),
                duration: Duration::from_secs(2),
                out: PathBuf::from("out.wav"),
            }))
        );
    }

    #[test]
    fn parses_record_command_with_short_flags() {
        let command = parse_args([
            "clearline-lab",
            "record",
            "-d",
            "1",
            "-s",
            "5",
            "-o",
            "artifacts/lab/reference.wav",
        ]);

        assert_eq!(
            command,
            Ok(Command::Record(RecordOptions {
                device_query: "1".to_owned(),
                duration: Duration::from_secs(5),
                out: PathBuf::from("artifacts/lab/reference.wav"),
            }))
        );
    }

    #[test]
    fn rejects_missing_record_fields() {
        let error = parse_args(["clearline-lab", "record", "--device", "mic"])
            .expect_err("missing fields should fail");

        assert!(error.contains("missing --seconds"), "{error}");
    }

    #[test]
    fn rejects_zero_seconds() {
        let error = parse_args([
            "clearline-lab",
            "record",
            "--device",
            "mic",
            "--seconds",
            "0",
            "--out",
            "out.wav",
        ])
        .expect_err("zero seconds should fail");

        assert!(error.contains("greater than zero"), "{error}");
    }
}
