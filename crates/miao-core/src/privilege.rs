use tracing::error;

pub fn require_privileges() {
    if !nix::unistd::Uid::effective().is_root() {
        error!("This application must be run as root");
        std::process::exit(1);
    }
}
