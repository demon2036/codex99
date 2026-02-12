use anyhow::Context;
use std::path::PathBuf;
use std::process::Command;

fn codex_sibling_path() -> anyhow::Result<PathBuf> {
    let exe = std::env::current_exe().context("failed to read current executable path")?;
    let ext = exe
        .extension()
        .map(|ext| format!(".{}", ext.to_string_lossy()));
    let filename = match ext {
        Some(ext) => format!("codex{ext}"),
        None => "codex".to_string(),
    };
    Ok(exe.with_file_name(filename))
}

fn resolve_coder_home() -> anyhow::Result<PathBuf> {
    if let Some(custom_home) = std::env::var_os("CODER_HOME") {
        return Ok(PathBuf::from(custom_home));
    }

    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .context("failed to determine home directory for coder")?;

    Ok(PathBuf::from(home).join(".coder"))
}

fn ensure_isolated_coder_home() -> anyhow::Result<()> {
    if std::env::var_os("CODEX_HOME").is_some() {
        return Ok(());
    }

    let coder_home = resolve_coder_home()?;
    std::fs::create_dir_all(&coder_home)
        .with_context(|| format!("failed to create coder home at {}", coder_home.display()))?;

    unsafe {
        std::env::set_var("CODEX_HOME", &coder_home);
    }

    Ok(())
}

fn main() -> anyhow::Result<()> {
    unsafe {
        std::env::set_var("CODEX_LAUNCHED_FROM_CODER", "1");
    }
    ensure_isolated_coder_home()?;

    let mut args = std::env::args_os();
    let _ = args.next();

    let codex_candidate = codex_sibling_path()?;
    let mut cmd = if codex_candidate.is_file() {
        Command::new(codex_candidate)
    } else {
        Command::new("codex")
    };
    cmd.args(args);

    let status = cmd.status().context("failed to execute codex binary")?;
    std::process::exit(status.code().unwrap_or(1));
}
