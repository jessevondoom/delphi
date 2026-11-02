class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.5.1"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.1/delphi-v0.5.1-aarch64-apple-darwin.tar.gz"
      sha256 "a13aae4b5d8c81163502d89381a7f19ff21bb1b223ac3b77da89d85d906f950e"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.1/delphi-v0.5.1-x86_64-apple-darwin.tar.gz"
      sha256 "c7340a3f86c63b92b34651605cd36c87e70f7e365e2e54913ff905a775e72567"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.1/delphi-v0.5.1-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "941daa03ac1369c55082eee626029c0372aed4dcc3b2e0a9f94bac6bd4a039a7"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.1/delphi-v0.5.1-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "d8c87b18e15ecc19fc633f4a9ae1eb91f35932ca33a70510abba7b45beca7dea"
    end
  end

  def install
    bin.install "delphi", "delphi-ui"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
