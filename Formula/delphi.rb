class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.2.1"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.1/delphi-v0.2.1-aarch64-apple-darwin.tar.gz"
      sha256 "75beddec2a829aba4ba0bca57d89be8f22addebfcd42a9e9dd27a53b343a89d5"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.1/delphi-v0.2.1-x86_64-apple-darwin.tar.gz"
      sha256 "cb0b98bf14c2ef571005208f60a960925ffd33b12d8185df3a8e447143698ec2"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.1/delphi-v0.2.1-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "155262069f89428e6ef49ac764862b4945ed4f00cf3e776e6556f2c18b37918a"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.1/delphi-v0.2.1-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "038663bc07f6d447eaa146e01d736ac91c4cc8c98ceb7b71892c9a56d3ad32fe"
    end
  end

  def install
    bin.install "delphi", "delphi-ui"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
