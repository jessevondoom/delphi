class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.2.5"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.5/delphi-v0.2.5-aarch64-apple-darwin.tar.gz"
      sha256 "eee9489ca5c0ae14ffb24b648ba0aaa30259bd7775cb5f47b0b92ea94a4f1e72"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.5/delphi-v0.2.5-x86_64-apple-darwin.tar.gz"
      sha256 "3578cae453a9947ca2689e0fe6378658cca1fb2e78acabd081df918bf7d4bd05"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.5/delphi-v0.2.5-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "31b6b8f5cb2e8ce8799733beb300b4ab437621288638188254e2d8b0d281ec32"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.5/delphi-v0.2.5-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "056500f8979b621a7151b90b9744617b8d731292f287eb8445b3084af9e95483"
    end
  end

  def install
    bin.install "delphi", "delphi-ui"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
