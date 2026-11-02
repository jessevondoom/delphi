class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.5.2"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.2/delphi-v0.5.2-aarch64-apple-darwin.tar.gz"
      sha256 "9693ef674daa1671174481a92558263d663d7f6fc7e9d4dcb65bc3cf0f81a07e"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.2/delphi-v0.5.2-x86_64-apple-darwin.tar.gz"
      sha256 "8a6cb033d4f818cb26a1ca5d2621ceabec9138838dd6aba3a19a8bebb655af7e"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.2/delphi-v0.5.2-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "e1f6db178dbb44a7f71ae09d085e37119041bca43873063f590812d1efdec23a"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.2/delphi-v0.5.2-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "cc98504c643941ef9c4d77dc820be8a3821a52c9cae8b90aefd48e7a4db33505"
    end
  end

  def install
    bin.install "delphi", "delphi-ui"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
