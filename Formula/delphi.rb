class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.1.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.1.0/delphi-v0.1.0-aarch64-apple-darwin.tar.gz"
      sha256 "5cd24611f202f3df81f110ba59dad6987a8acd97e4aed32eed4bd1d39cc5f2eb"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.1.0/delphi-v0.1.0-x86_64-apple-darwin.tar.gz"
      sha256 "03231b5550cf7e3386492da28247c8e31d7cc2e77e4abaa12e18de26f1f26e54"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.1.0/delphi-v0.1.0-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "939beeb61f392edbbf50286d0f0e05e6c8512bfc7100f3a627575fe2962062a9"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.1.0/delphi-v0.1.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "aa117dafc4ac7e86912c16cec036582fc75dd94d5cf516ef2ff6b184d351d482"
    end
  end

  def install
    bin.install "delphi"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
