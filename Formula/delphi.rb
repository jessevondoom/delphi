class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.6.1"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.1/delphi-v0.6.1-aarch64-apple-darwin.tar.gz"
      sha256 "36aa3e7b62f7bab28c2abfc5cb9833a7bfd97b8d7d0f88b6bc075cf78f3dc7cd"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.1/delphi-v0.6.1-x86_64-apple-darwin.tar.gz"
      sha256 "5abbd4941189e805ba0250b26aae072b987058babf0d5babeca5fada62119363"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.1/delphi-v0.6.1-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "dd697ed4766416df0d31e2cfee9a4e031fdae67fc3b12098c0d2baf20dd748a7"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.1/delphi-v0.6.1-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "a25ba4bd19b9dd10f51e8b628be76b452cc6ad49c250b131f8c1923e681c3337"
    end
  end

  def install
    bin.install "delphi", "delphi-ui"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
