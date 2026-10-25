class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.2.3"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.3/delphi-v0.2.3-aarch64-apple-darwin.tar.gz"
      sha256 "daa60a06306cc643a6cdd97b692b6f8f1594980d31149dc9ce55332fa8274fe9"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.3/delphi-v0.2.3-x86_64-apple-darwin.tar.gz"
      sha256 "c664fe689cd96e4736209e2f740fcc07c08e1c8dedead0943e3a42f88eac1458"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.3/delphi-v0.2.3-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "6b394f4ff72df5adf19a6b486566f290e0136690291209f08816f446c35ffb34"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.2.3/delphi-v0.2.3-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "b1621808b0835714023b51c643b8ada9eb2bc01a6646953c457c990b3f3c79c4"
    end
  end

  def install
    bin.install "delphi", "delphi-ui"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
