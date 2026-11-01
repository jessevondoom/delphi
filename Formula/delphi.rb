class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.5.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.0/delphi-v0.5.0-aarch64-apple-darwin.tar.gz"
      sha256 "9d5c1e36b61464a21cc1c2f060c42657d97412dc0167d0c0a334d386c8a8cd67"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.0/delphi-v0.5.0-x86_64-apple-darwin.tar.gz"
      sha256 "815850b80e6f686e44469d65b9be4c89e91c461bcadf43beac78074ccd6a15d6"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.0/delphi-v0.5.0-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "e0676de02e39c23a7763b92085c9832c9a47a9a389e2cdaa6121f6ee8289800d"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.0/delphi-v0.5.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "a18358ecc90489979bc7880e6b662c49c84d4905f2d70e470454e44e65c034fc"
    end
  end

  def install
    bin.install "delphi", "delphi-ui"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
