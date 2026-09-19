use std::process::{ExitStatus, Stdio};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::time::{timeout, Duration};

use crate::error::{AppError, AppResult};

const OUTPUT_LIMIT: usize = 16 * 1024;

pub(super) struct SshOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

// Drain both pipes while SSH runs, retaining only a bounded tail. Package
// managers can otherwise fill a pipe and deadlock before child.wait() returns.
async fn read_tail(mut stream: impl AsyncRead + Unpin) -> std::io::Result<Vec<u8>> {
    let mut tail = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let len = stream.read(&mut buffer).await?;
        if len == 0 {
            return Ok(tail);
        }
        tail.extend_from_slice(&buffer[..len]);
        if tail.len() > OUTPUT_LIMIT {
            tail.drain(..tail.len() - OUTPUT_LIMIT);
        }
    }
}

pub(super) async fn run_script(
    mut command: Command,
    script: &str,
    deadline: Duration,
    stage: &str,
) -> AppResult<SshOutput> {
    let mut child = command
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                AppError::message(
                    "本机未安装 SSH 客户端。请在运行 Miao 的设备上安装 OpenSSH 客户端后重试。",
                )
            } else {
                AppError::context("无法启动本机 SSH 客户端", err)
            }
        })?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let result = timeout(deadline, async {
        tokio::join!(
            async move {
                stdin.write_all(script.as_bytes()).await?;
                stdin.shutdown().await
            },
            read_tail(stdout),
            read_tail(stderr),
            child.wait(),
        )
    })
    .await;
    match result {
        Ok((sent, stdout, stderr, status)) => {
            let status = status.map_err(|err| AppError::context("等待 SSH 结束失败", err))?;
            // Authentication failures often also cause BrokenPipe; preserve the
            // SSH diagnostic instead of replacing it with a stdin write error.
            if status.success() {
                sent.map_err(|err| AppError::context("发送部署脚本失败", err))?;
            }
            Ok(SshOutput {
                status,
                stdout: stdout.map_err(|err| AppError::context("读取 SSH 输出失败", err))?,
                stderr: stderr.map_err(|err| AppError::context("读取 SSH 错误失败", err))?,
            })
        }
        Err(_) => {
            let _ = child.kill().await;
            Err(AppError::message(format!(
                "{stage}超时（{} 秒）。请检查 VPS 网络、软件源和 GitHub 下载是否可达。SSH 会话已终止，远端操作可能尚未完成，请确认服务状态后重试。",
                deadline.as_secs()
            )))
        }
    }
}

pub(super) fn failure(output: &SshOutput, stage: &str, secrets: &[&str]) -> AppError {
    let mut details = String::from_utf8_lossy(&output.stderr).into_owned();
    for secret in secrets.iter().filter(|secret| !secret.is_empty()) {
        details = details.replace(secret, "[已隐藏]");
    }
    let lower = details.to_ascii_lowercase();
    let authentication_failed = output.status.code() == Some(255)
        && (lower.contains("permission denied")
            || lower.contains("authentication failed")
            || lower.contains("no supported authentication methods"));
    let hint = if output.status.code() != Some(255) {
        details
            .lines()
            .rev()
            .find_map(|line| line.strip_prefix("MIAO_ERROR: "))
            .unwrap_or("远端操作失败。请查看下方错误详情，检查 VPS 环境后重试。")
    } else if lower.contains("remote host identification has changed")
        || lower.contains("host key verification failed")
    {
        "SSH 主机密钥校验失败。若 VPS 曾重装，请先通过服务商控制台核对主机指纹，再更新运行 Miao 账户的 known_hosts；不要直接关闭主机校验。"
    } else if authentication_failed {
        "SSH 认证被拒绝。当前部署使用 root 密码登录：请确认密码正确，且 VPS 允许 root 登录和密码认证。客户端无法区分密码错误与服务端禁用登录；请通过服务商控制台检查 sshd 的 PermitRootLogin、PasswordAuthentication、Include/Match 配置及认证日志。若仅允许密钥登录，当前密码部署方式不适用。"
    } else if lower.contains("connection refused") {
        "SSH 连接被拒绝。请确认 VPS 的 SSH 服务已启动并监听 22 端口；当前部署使用固定的 22 端口。"
    } else if lower.contains("connection timed out") || lower.contains("operation timed out") {
        "SSH 连接超时。请检查 VPS 地址、22 端口，以及服务商安全组和防火墙是否允许运行 Miao 的设备连接。"
    } else if lower.contains("no route to host") || lower.contains("network is unreachable") {
        "无法到达 VPS。请检查地址、网络路由，以及本机是否具备对应的 IPv4/IPv6 连通性。"
    } else if lower.contains("could not resolve hostname") {
        "无法解析 VPS 地址。请检查域名拼写及运行 Miao 设备的 DNS。"
    } else if lower.contains("connection reset")
        || lower.contains("connection closed")
        || lower.contains("broken pipe")
    {
        "SSH 连接被中断。请检查 VPS 的 SSH 服务日志、连接限制或封禁策略（如 Fail2ban），并确认网络稳定后重试。"
    } else {
        details
            .lines()
            .rev()
            .find_map(|line| line.strip_prefix("MIAO_ERROR: "))
            .unwrap_or("远端操作失败。请查看下方错误详情，检查 VPS 环境后重试。")
    };
    let clean: String = details
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect();
    let detail: String = clean
        .chars()
        .rev()
        .take(4000)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let help = if authentication_failed {
        format!("\n\n{}", include_str!("ssh-auth-help.txt").trim())
    } else {
        String::new()
    };
    AppError::message(format!(
        "{stage}失败：{hint}{help}\n\n错误详情（退出状态 {}）：\n{}",
        output.status,
        if detail.trim().is_empty() {
            "SSH 未返回错误信息"
        } else {
            detail.trim()
        }
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    fn output(stderr: &str, code: i32) -> SshOutput {
        SshOutput {
            status: ExitStatus::from_raw(code << 8),
            stdout: vec![],
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    #[test]
    fn ssh_errors_explain_the_next_step_without_guessing_auth_policy() {
        for (raw, hint) in [
            (
                "root@vps: Permission denied (publickey,password).",
                "客户端无法区分",
            ),
            (
                "ssh: connect to host vps port 22: Connection refused",
                "监听 22 端口",
            ),
            (
                "ssh: connect to host vps port 22: Connection timed out",
                "安全组",
            ),
            (
                "WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!",
                "核对主机指纹",
            ),
            ("ssh: Could not resolve hostname vps", "DNS"),
            ("ssh: connect: Network is unreachable", "IPv4/IPv6"),
            ("Connection reset by peer", "Fail2ban"),
        ] {
            let error = failure(&output(raw, 255), "检查 VPS 环境", &[]).to_string();
            assert!(error.contains(hint), "{error}");
            assert!(error.contains(raw));
        }
    }

    #[test]
    fn remote_stage_errors_preserve_actionable_details_and_redact_secrets() {
        let error = failure(&output("curl: Connection refused\nMIAO_ERROR: 安装依赖失败，请检查软件源。\nsecret-root\nsecret-node", 1), "安装 Hysteria2", &["secret-root", "secret-node"]).to_string();
        assert!(error.contains("安装依赖失败，请检查软件源。"));
        assert!(!error.contains("secret-root"));
        assert!(!error.contains("secret-node"));
    }

    #[test]
    fn auth_failures_include_conditional_cloud_init_recovery_steps() {
        let error = failure(
            &output("Permission denied (publickey).", 255),
            "检查 VPS 环境",
            &[],
        )
        .to_string();
        assert!(error.contains("目标 VPS 上以 root 执行"));
        assert!(error.contains("sshd -t && sshd -T"));
        assert!(error.contains("50-cloud-init.conf"));
        assert!(error.contains("仅当已确认"));
        assert!(error.contains("(set -C; printf"));
        assert!(error.contains("systemctl reload sshd || systemctl reload ssh"));
        assert!(error.contains("rc-service sshd reload"));
        assert!(error.contains("journalctl -u sshd -u ssh"));
        let raw = error.find("错误详情（退出状态").unwrap();
        assert!(error.find("解决办法：").unwrap() < raw);
        for (stderr, code) in [
            ("Connection refused", 255),
            ("Permission denied", 1),
            ("Host key verification failed", 255),
        ] {
            assert!(!failure(&output(stderr, code), "检查 VPS 环境", &[])
                .to_string()
                .contains("00-miao-password-auth.conf"));
        }
    }

    #[tokio::test]
    async fn script_receives_eof_and_large_stderr_does_not_deadlock() {
        let mut command = Command::new("sh");
        command.arg("-s");
        let output = run_script(
            command,
            "head -c 131072 /dev/zero >&2\nprintf 'credentials\\n'\n",
            Duration::from_secs(5),
            "测试",
        )
        .await
        .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"credentials\n");
        assert_eq!(output.stderr.len(), OUTPUT_LIMIT);
    }

    #[tokio::test]
    async fn authentication_error_survives_broken_stdin() {
        let mut command = Command::new("sh");
        command.args(["-c", "echo 'Permission denied (publickey).' >&2; exit 255"]);
        let output = run_script(
            command,
            &"x".repeat(1024 * 1024),
            Duration::from_secs(5),
            "测试",
        )
        .await
        .unwrap();
        assert_eq!(output.status.code(), Some(255));
        assert!(failure(&output, "测试", &[])
            .to_string()
            .contains("仅允许密钥登录"));
    }

    #[tokio::test]
    async fn timeout_covers_script_delivery_too() {
        let mut command = Command::new("sh");
        command.args(["-c", "exec sleep 10"]);
        let result = run_script(
            command,
            &"x".repeat(1024 * 1024),
            Duration::from_millis(50),
            "检查 VPS 环境",
        )
        .await;
        assert!(result.err().unwrap().to_string().contains("SSH 会话已终止"));
    }
}
