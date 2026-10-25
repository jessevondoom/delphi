class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.2.6"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.6/delphi-v0.2.6-aarch64-apple-darwin.tar.gz"
      sha256 "25bfad6e296a94a6c471692c57f84a829932f6de5a46d71f1f16cf975203a7ea"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.6/delphi-v0.2.6-x86_64-apple-darwin.tar.gz"
      sha256 "0948f2c08aecf7199a9ed89d2064535fa6927d13110a69a4fd9892110a5cc3cd"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.6/delphi-v0.2.6-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "dead168cca380dd99c8ea19ce03b2e6e4807e8fe2d03d72a128eb3d08647e21b"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.6/delphi-v0.2.6-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "e909b1e5291e4e778e8b16f50b905be296dc761348a33d6b728d301c25f66428"
    end
  end

  def install
    bin.install "delphi", "delphi-ui"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
