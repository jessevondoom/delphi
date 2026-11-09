class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.6.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.0/delphi-v0.6.0-aarch64-apple-darwin.tar.gz"
      sha256 "4ecbe53eb22139495384a2baf1feb7410ad454d342a155323d3937fe40099fd3"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.0/delphi-v0.6.0-x86_64-apple-darwin.tar.gz"
      sha256 "28aea0b6e1a6c556b4dd6764fba0d496419db335e0bdc14436735a190d930937"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.0/delphi-v0.6.0-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "3caf0b8852dbd11d68b90e861190e2347939b130feedb976dbc07fefd50790d4"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.0/delphi-v0.6.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "c66066f242c911b89e14d9c053018d44140abe82c57745bc3d814ada7114df79"
    end
  end

  def install
    bin.install "delphi", "delphi-ui"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
