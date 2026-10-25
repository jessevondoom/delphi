class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.2.4"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.4/delphi-v0.2.4-aarch64-apple-darwin.tar.gz"
      sha256 "2a0a3f1dcf465f43dd539c72a5c174f19ac4bb1c566746bc15de441716355423"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.4/delphi-v0.2.4-x86_64-apple-darwin.tar.gz"
      sha256 "c611474d125e52f1cd21375c06b4b0f017285f916e3e4186e3f3e79e7c84b617"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.4/delphi-v0.2.4-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "feef90f4f7b162265ea3e1d9e2bbd25ad0f77671fce4dd335d049c283e4640d4"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.4/delphi-v0.2.4-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "5b791fb874d144fc651b413af1f58008b9fb7cd497c196d5c45f23dc7b18e105"
    end
  end

  def install
    bin.install "delphi", "delphi-ui"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
