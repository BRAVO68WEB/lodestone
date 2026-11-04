use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use axum::{extract::Path as AxumPath, Json};
use axum_auth::AuthBearer;
use color_eyre::eyre::eyre;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use crate::auth::user::UserAction;
use crate::error::{Error, ErrorKind};
use crate::events::{CausedBy, Event, EventInner, InstanceEvent, InstanceEventInner};
use crate::traits::t_configurable::TConfigurable;
use crate::types::{InstanceUuid, Snowflake};
use crate::util::dont_spawn_terminal;
use crate::AppState;

const ALLOWED: &[&str] = &[
    "cat", "cp", "mv", "mkdir", "ls", "rm", "zip", "unzip", "curl", "wget",
];
const MAX_OUTPUT: usize = 200 * 1024;
const TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Deserialize)]
pub struct ShellCommandBody {
    pub command: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ShellOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub truncated: bool,
}

#[derive(Debug)]
pub struct ShellRunError {
    pub rejected: bool,
    pub message: String,
}

pub async fn run_instance_shell(
    axum::extract::State(state): axum::extract::State<AppState>,
    AxumPath(uuid): AxumPath<InstanceUuid>,
    AuthBearer(token): AuthBearer,
    Json(body): Json<ShellCommandBody>,
) -> Result<Json<ShellOutput>, Error> {
    let requester = state.users_manager.read().await.try_auth_or_err(&token)?;
    let safe_mode = state.global_settings.lock().await.safe_mode();
    requester.try_action(&UserAction::InstanceShell(uuid.clone()), safe_mode)?;

    let instance = state
        .instances
        .get(&uuid)
        .ok_or_else(|| Error {
            kind: ErrorKind::NotFound,
            source: eyre!("Instance not found"),
        })?
        .clone();
    let root = instance.path().await;
    let name = instance.name().await;
    let user_name = requester.username.clone();
    let outcome = run_shell_line(&root, &body.command).await;
    let record = match &outcome {
        Ok(output) => {
            let code = output
                .exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "none".to_string());
            format!("{user_name} ran `{}` (exit {code})", body.command)
        }
        Err(error) => format!("{user_name} ran `{}`: {}", body.command, error.message),
    };
    state.event_broadcaster.send(Event {
        event_inner: EventInner::InstanceEvent(InstanceEvent {
            instance_uuid: uuid,
            instance_name: name,
            instance_event_inner: InstanceEventInner::InstanceInput { message: record },
        }),
        details: body.command,
        snowflake: Snowflake::default(),
        caused_by: CausedBy::User {
            user_id: requester.uid,
            user_name,
        },
    });
    match outcome {
        Ok(output) => Ok(Json(output)),
        Err(error) => Err(Error {
            kind: if error.rejected {
                ErrorKind::BadRequest
            } else {
                ErrorKind::Internal
            },
            source: eyre!(error.message),
        }),
    }
}

pub fn split_command(input: &str) -> Result<Vec<String>, ShellRunError> {
    if input.chars().count() > 4_000 {
        return Err(reject("Command is too long."));
    }
    if input.contains('`') {
        return Err(reject(
            "Backticks are not allowed. This is not a login shell.",
        ));
    }
    if input.contains('\n') || input.contains('\r') {
        return Err(reject("Command must be a single line."));
    }
    let mut args = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut quote: Option<char> = None;
    let mut in_arg = false;
    while let Some(character) = chars.next() {
        if let Some(active) = quote {
            if character == active {
                quote = None;
            } else {
                current.push(character);
            }
            in_arg = true;
            continue;
        }
        match character {
            '"' | '\'' => {
                quote = Some(character);
                in_arg = true;
            }
            ' ' | '\t' => {
                if in_arg {
                    args.push(std::mem::take(&mut current));
                    in_arg = false;
                }
            }
            '|' | '>' | '<' | ';' | '&' => {
                return Err(reject(format!(
                    "Shell syntax '{character}' is not allowed. This is not a login shell."
                )));
            }
            _ => {
                current.push(character);
                in_arg = true;
            }
        }
    }
    if quote.is_some() {
        return Err(reject("Unclosed quote."));
    }
    if in_arg {
        args.push(current);
    }
    if args.is_empty() {
        return Err(reject("Enter a command."));
    }
    if args.len() > 64 {
        return Err(reject("Too many arguments."));
    }
    Ok(args)
}

pub fn plan_shell_line(root: &Path, line: &str) -> Result<(String, Vec<String>), ShellRunError> {
    let args = split_command(line)?;
    let program = args[0].as_str();
    if program.contains('/') || program.contains('\\') || !ALLOWED.contains(&program) {
        return Err(reject(format!(
            "'{program}' is not allowed. Allowed commands: {}.",
            ALLOWED.join(", ")
        )));
    }
    let jailed = jail_arguments(root, program, &args[1..])?;
    Ok((program.to_string(), jailed))
}

pub async fn run_shell_line(root: &Path, line: &str) -> Result<ShellOutput, ShellRunError> {
    let (program, args) = plan_shell_line(root, line)?;
    let binary = program_path(&program, root)?;
    execute(root, binary, args).await
}

pub fn jail_arguments(
    root: &Path,
    program: &str,
    args: &[String],
) -> Result<Vec<String>, ShellRunError> {
    let root = root.canonicalize().map_err(|error| {
        reject(format!("Instance directory is not available: {error}"))
    })?;
    let mut rewritten = Vec::with_capacity(args.len());
    for arg in args {
        if is_http_url(arg) {
            if !url_program(program) {
                return Err(reject("Only curl and wget can take a URL."));
            }
            rewritten.push(arg.clone());
            continue;
        }
        if arg.contains("://") {
            return Err(reject("Only http:// and https:// URLs are allowed."));
        }
        if arg == "-" {
            rewritten.push(arg.clone());
            continue;
        }
        if let Some(stripped) = arg.strip_prefix('-') {
            if let Some((flag, value)) = split_flag_value(arg) {
                if is_http_url(value) {
                    if !url_program(program) {
                        return Err(reject("Only curl and wget can take a URL."));
                    }
                    rewritten.push(arg.clone());
                    continue;
                }
                if value.contains("://") {
                    return Err(reject("Only http:// and https:// URLs are allowed."));
                }
                if value.is_empty() || !value_needs_jail(value) {
                    rewritten.push(arg.clone());
                    continue;
                }
                let path = resolve_inside(&root, value)?;
                refuse_root(program, &root, &path)?;
                rewritten.push(format!("{flag}={}", path.display()));
                continue;
            }
            if stripped.contains('/') || stripped.contains('\\') || stripped.contains("..") {
                return Err(reject(
                    "Flags cannot embed a path. Pass the path as its own argument.",
                ));
            }
            rewritten.push(arg.clone());
            continue;
        }
        let path = resolve_inside(&root, arg)?;
        refuse_root(program, &root, &path)?;
        rewritten.push(path.display().to_string());
    }
    Ok(rewritten)
}

fn reject(message: impl Into<String>) -> ShellRunError {
    ShellRunError {
        rejected: true,
        message: message.into(),
    }
}

fn url_program(program: &str) -> bool {
    program == "curl" || program == "wget"
}

fn is_http_url(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

fn split_flag_value(arg: &str) -> Option<(&str, &str)> {
    let (flag, value) = arg.split_once('=')?;
    if flag.starts_with('-') {
        Some((flag, value))
    } else {
        None
    }
}

fn value_needs_jail(value: &str) -> bool {
    value.starts_with('.')
        || value.contains('/')
        || value.contains('\\')
        || value.contains("..")
        || Path::new(value).is_absolute()
}

fn refuse_root(program: &str, root: &Path, path: &Path) -> Result<(), ShellRunError> {
    if (program == "rm" || program == "mv") && path == root {
        return Err(reject(
            "Refusing to target the instance directory itself.",
        ));
    }
    Ok(())
}

fn resolve_inside(root: &Path, raw: &str) -> Result<PathBuf, ShellRunError> {
    if raw.contains('\0') {
        return Err(reject("Invalid path."));
    }
    let lexical = lexical_normalize(root, raw)?;
    if !lexical.starts_with(root) {
        return Err(reject("That path is outside the instance directory."));
    }
    if lexical.exists() {
        let canonical = lexical.canonicalize().map_err(|error| {
            reject(format!("Could not resolve path: {error}"))
        })?;
        if !canonical.starts_with(root) {
            return Err(reject("That path is outside the instance directory."));
        }
        return Ok(canonical);
    }
    if let Some(parent) = lexical.parent() {
        if parent.exists() {
            let canonical_parent = parent.canonicalize().map_err(|error| {
                reject(format!("Could not resolve path: {error}"))
            })?;
            if !canonical_parent.starts_with(root) {
                return Err(reject("That path is outside the instance directory."));
            }
        }
    }
    Ok(lexical)
}

fn lexical_normalize(root: &Path, raw: &str) -> Result<PathBuf, ShellRunError> {
    let raw_path = Path::new(raw);
    let combined = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        root.join(raw_path)
    };
    let mut out = PathBuf::new();
    for component in combined.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => out.push(component),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return Err(reject("That path is outside the instance directory."));
                }
            }
            Component::Normal(part) => out.push(part),
        }
    }
    if !out.starts_with(root) {
        return Err(reject("That path is outside the instance directory."));
    }
    Ok(out)
}

fn program_path(name: &str, instance_root: &Path) -> Result<PathBuf, ShellRunError> {
    let mut dirs = vec![
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/opt/homebrew/bin"),
    ];
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    for dir in dirs {
        if dir.starts_with(instance_root) {
            continue;
        }
        let candidate = dir.join(name);
        if !candidate.is_file() {
            continue;
        }
        if let Ok(canonical) = candidate.canonicalize() {
            if !canonical.starts_with(instance_root) {
                return Ok(canonical);
            }
        }
    }
    Err(reject(format!("{name} is not installed on this Core.")))
}

async fn execute(
    root: &Path,
    program: PathBuf,
    args: Vec<String>,
) -> Result<ShellOutput, ShellRunError> {
    let mut command = Command::new(&program);
    command
        .current_dir(root)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    dont_spawn_terminal(&mut command);
    let mut child = command.spawn().map_err(|error| ShellRunError {
        rejected: false,
        message: format!("Failed to start {}: {error}", program.display()),
    })?;
    let mut stdout = child.stdout.take().ok_or_else(|| ShellRunError {
        rejected: false,
        message: "Failed to capture stdout.".into(),
    })?;
    let mut stderr = child.stderr.take().ok_or_else(|| ShellRunError {
        rejected: false,
        message: "Failed to capture stderr.".into(),
    })?;
    let outcome = tokio::time::timeout(TIMEOUT, async {
        let (out, err, status) = tokio::join!(
            read_capped(&mut stdout, MAX_OUTPUT),
            read_capped(&mut stderr, MAX_OUTPUT),
            child.wait()
        );
        (out, err, status)
    })
    .await;
    let (out, err, status) = outcome.map_err(|_| ShellRunError {
        rejected: false,
        message: "Command timed out after 60 seconds.".into(),
    })?;
    let (out_bytes, out_truncated) = out.map_err(|error| ShellRunError {
        rejected: false,
        message: format!("Failed to read stdout: {error}"),
    })?;
    let (err_bytes, err_truncated) = err.map_err(|error| ShellRunError {
        rejected: false,
        message: format!("Failed to read stderr: {error}"),
    })?;
    let status = status.map_err(|error| ShellRunError {
        rejected: false,
        message: format!("Command failed: {error}"),
    })?;
    Ok(ShellOutput {
        stdout: finish_output(&out_bytes, out_truncated),
        stderr: finish_output(&err_bytes, err_truncated),
        exit_code: status.code(),
        truncated: out_truncated || err_truncated,
    })
}

async fn read_capped<R>(reader: &mut R, cap: usize) -> std::io::Result<(Vec<u8>, bool)>
where
    R: AsyncRead + Unpin,
{
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut truncated = false;
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        let room = cap.saturating_sub(buf.len());
        if room == 0 {
            truncated = true;
            continue;
        }
        if read > room {
            buf.extend_from_slice(&chunk[..room]);
            truncated = true;
        } else {
            buf.extend_from_slice(&chunk[..read]);
        }
    }
    Ok((buf, truncated))
}

fn finish_output(bytes: &[u8], truncated: bool) -> String {
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    if truncated {
        text.push_str("\n[output truncated]\n");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_instance() -> tempdir::TempDir {
        tempdir::TempDir::new("lodestone-shell").unwrap()
    }

    #[test]
    fn quotes_keep_spaces_and_quoted_ampersands() {
        let args = split_command("cat \"my file.txt\" 'a&b'").unwrap();
        assert_eq!(args, vec!["cat", "my file.txt", "a&b"]);
    }

    #[test]
    fn unquoted_shell_syntax_is_rejected() {
        assert!(split_command("cat file | nc").is_err());
        assert!(split_command("cat file > out").is_err());
        assert!(split_command("cat `id`").is_err());
        assert!(split_command("bash -c id").unwrap()[0] == "bash");
    }

    #[test]
    fn bash_is_rejected() {
        let dir = temp_instance();
        let error = plan_shell_line(dir.path(), "bash -c id").unwrap_err();
        assert!(error.rejected);
        assert!(error.message.contains("not allowed"));
    }

    #[test]
    fn cat_parent_is_rejected() {
        let dir = temp_instance();
        let error = plan_shell_line(dir.path(), "cat ../").unwrap_err();
        assert!(error.message.contains("outside"));
    }

    #[test]
    fn curl_output_stays_inside_and_file_urls_are_rejected() {
        let dir = temp_instance();
        let root = dir.path().canonicalize().unwrap();
        let (program, args) =
            plan_shell_line(dir.path(), "curl https://example.com -o download.txt").unwrap();
        assert_eq!(program, "curl");
        assert_eq!(args[0], "https://example.com");
        assert_eq!(args[1], "-o");
        assert!(args[2].ends_with("download.txt"));
        assert!(Path::new(&args[2]).starts_with(&root));

        assert!(plan_shell_line(dir.path(), "curl file:///etc/passwd").is_err());
        assert!(plan_shell_line(dir.path(), "curl https://example.com -o ../download.txt").is_err());
        assert!(plan_shell_line(dir.path(), "wget http://example.com -O ../out").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_outside_the_instance_is_rejected() {
        let dir = temp_instance();
        let outside = temp_instance();
        std::fs::write(outside.path().join("secret"), "nope").unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret"), dir.path().join("link")).unwrap();
        let error = plan_shell_line(dir.path(), "cat link").unwrap_err();
        assert!(error.message.contains("outside"));
    }

    #[test]
    fn truncation_marker_is_appended() {
        assert_eq!(
            finish_output(b"abc", true),
            "abc\n[output truncated]\n"
        );
        assert_eq!(finish_output(b"abc", false), "abc");
    }

    #[tokio::test]
    async fn capped_read_marks_only_extra_bytes() {
        let mut small = std::io::Cursor::new(vec![b'a'; 40]);
        let (buf, truncated) = read_capped(&mut small, 40).await.unwrap();
        assert_eq!(buf.len(), 40);
        assert!(!truncated);

        let mut large = std::io::Cursor::new(vec![b'b'; 100]);
        let (buf, truncated) = read_capped(&mut large, 40).await.unwrap();
        assert_eq!(buf.len(), 40);
        assert!(truncated);
        assert!(buf.iter().all(|byte| *byte == b'b'));
    }

    #[tokio::test]
    async fn cat_reads_a_file_inside_the_instance() {
        let dir = temp_instance();
        std::fs::write(dir.path().join("hello.txt"), "hi\n").unwrap();
        if program_path("cat", dir.path()).is_err() {
            return;
        }
        let output = run_shell_line(dir.path(), "cat hello.txt").await.unwrap();
        assert_eq!(output.exit_code, Some(0));
        assert!(output.stdout.contains("hi"));
        assert!(!output.truncated);
    }
}
