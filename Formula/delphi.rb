class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.5.4"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.4/delphi-v0.5.4-aarch64-apple-darwin.tar.gz"
      sha256 "b7a9a8838ba385941f2e9f9ac1a417a45eefdf3770d97a6851545ad0fa00d70b"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.4/delphi-v0.5.4-x86_64-apple-darwin.tar.gz"
      sha256 "118c6ceb3bf1e51fb9b6fbe042dc21be080a93858303cb6672b148177feb4f40"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.4/delphi-v0.5.4-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "e4e2c3dc9ed22deb24f7f1eaadd54d99b9b81537c0dacff5d739809fb8b6a0b2"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.4/delphi-v0.5.4-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "45b6868dd499794e430dda0f717e327ebf2db44fc0b177b6f553bd80f6743d04"
    end
  end

  def install
    bin.install "delphi", "delphi-ui"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
