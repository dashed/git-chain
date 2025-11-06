use git2::Error;

// For API consistency, we create our own Error variants
pub trait ErrorExt {
    #[allow(dead_code)]
    fn from_str(message: &str) -> Self;
    fn merge_conflict(branch: String, upstream: String, message: Option<String>) -> Self;
    fn git_command_failed(command: String, status: i32, stdout: String, stderr: String) -> Self;
}

impl ErrorExt for Error {
    fn from_str(message: &str) -> Self {
        Error::from_str(message)
    }

    fn merge_conflict(branch: String, upstream: String, message: Option<String>) -> Self {
        let mut error_msg = format!("Merge conflict between {} and {}", upstream, branch);
        if let Some(details) = message {
            error_msg.push('\n');
            error_msg.push_str(&details);
        }
        Error::from_str(&error_msg)
    }

    fn git_command_failed(command: String, status: i32, stdout: String, stderr: String) -> Self {
        let error_msg = format!(
            "Git command failed: {}\nStatus: {}\nStdout: {}\nStderr: {}",
            command, status, stdout, stderr
        );
        Error::from_str(&error_msg)
    }
}
