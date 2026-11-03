class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.5.3"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.3/delphi-v0.5.3-aarch64-apple-darwin.tar.gz"
      sha256 "697b7e69806712cef5ef97057a17fa4ee5f4b47fdfc836ab1d0cc681649ffba2"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.3/delphi-v0.5.3-x86_64-apple-darwin.tar.gz"
      sha256 "22348e7e1bed312652feb31558aa8b11794c5039c4c12ba7a3636acd4dfb14f3"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.3/delphi-v0.5.3-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "0cf04ef9dd95075ef88eb40f73a957d065005320920a4efc2d52db7cd934b5a0"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.3/delphi-v0.5.3-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "978601b36f8b2cd0325e010a9e17f581ca4db717e7de0ba435c81cca8b1fb3c8"
    end
  end

  def install
    bin.install "delphi", "delphi-ui"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
