class Superline < Formula
  desc "Configurable Powerline implementation in pure Rust"
  homepage "https://github.com/alxhill/superline"
  url "https://static.crates.io/crates/superline/superline-0.9.2.crate"
  sha256 "b1373856ded11ecb1b54f8923c09d9c81462f23205ac7c66347f48cc7fffb37b"
  license "MIT"
  head "https://github.com/alxhill/superline.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/superline --version")
    assert_match "function _update_ps1", shell_output("#{bin}/superline init bash")
  end
end
