class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.2.2"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.2/delphi-v0.2.2-aarch64-apple-darwin.tar.gz"
      sha256 "200c9d995bd3275a19af3100d85328ecb181ea88da1935bb6b0911c5b64c61c7"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.2/delphi-v0.2.2-x86_64-apple-darwin.tar.gz"
      sha256 "3c57acbc0fae24bd346effef3fb1b5c172ba9bf481ecb5e8b2aad4d03fdc157e"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.2/delphi-v0.2.2-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "e57f65590754716b3f6df8b8bdc26be7652154027fdc97682c080ed1d9b7f391"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.2/delphi-v0.2.2-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "e66cc1f2dc0cb099d6418800c819e3bf5a07ed7296d120f94c591ff24894ae1a"
    end
  end

  def install
    bin.install "delphi", "delphi-ui"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
