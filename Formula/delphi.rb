class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.3.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.3.0/delphi-v0.3.0-aarch64-apple-darwin.tar.gz"
      sha256 "8bacb75387d2811a9ded93b03bb682dae23d9323e00fc2d5f9dca32f11bdc120"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.3.0/delphi-v0.3.0-x86_64-apple-darwin.tar.gz"
      sha256 "a62025021446cf8a82fb8db09f76821a7baad63c85bce7689583aeb92e612308"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.3.0/delphi-v0.3.0-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "081a52f686da27bad7cbf74eaa84246e4e2ad1a374291d771c956ecd3efbd995"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.3.0/delphi-v0.3.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "5edaac9f9f580408343dfe6da80010e64ee10db1a714b87ab7f153485ac9f99f"
    end
  end

  def install
    bin.install "delphi", "delphi-ui"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
