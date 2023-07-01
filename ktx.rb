class Ktx < Formula
  desc "Kubernetes config manager"
  homepage "https://github.com/bsod90/ktx"
  url "https://github.com/bsod90/ktx/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "your_sha256_here"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "build", "--release", "--bin", "ktx"
    bin.install "target/release/ktx"
  end

  test do
    system "#{bin}/ktx", "--version"
  end
end

