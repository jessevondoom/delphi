class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.2.7"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.7/delphi-v0.2.7-aarch64-apple-darwin.tar.gz"
      sha256 "bd93891017d4f307a68a9fc7392bd563e6463b63ecd50b41d32064442eabd5f3"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.7/delphi-v0.2.7-x86_64-apple-darwin.tar.gz"
      sha256 "6c5096bf570be40f4622cd5e7e2d070388241b8ab1077a34aa0b15bcce68a4bd"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.7/delphi-v0.2.7-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "ec8b443e2f2fed8e060ce8fe87f5b5f37bca87e417694b7600e01596cf6bdbc7"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.7/delphi-v0.2.7-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "15a1ba6be52fc1bde74c0404da097d6e2e2000b4dd242e7d7bbd4c5dd23158ab"
    end
  end

  def install
    bin.install "delphi", "delphi-ui"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
