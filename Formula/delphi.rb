class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.4.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.4.0/delphi-v0.4.0-aarch64-apple-darwin.tar.gz"
      sha256 "1844f1226dfbf45f0f966f5fc284c165d9d24dc4d7c49706c56f6024d1bdb331"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.4.0/delphi-v0.4.0-x86_64-apple-darwin.tar.gz"
      sha256 "e7e80fd3a8678a3f5998b732eb35503a844a974345c962d56287a3ce32bc164c"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.4.0/delphi-v0.4.0-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "7922784d51b05021c2abefcdb6aed1d7769e6def0b84a0da0b2abcef47c466dc"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.4.0/delphi-v0.4.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "9bdeff44e5793ad8f213765c10e7d50cfc95e309ee14fec97e8a5f56c83bc20d"
    end
  end

  def install
    bin.install "delphi", "delphi-ui"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
