class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.5.5"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.5/delphi-v0.5.5-aarch64-apple-darwin.tar.gz"
      sha256 "6a3d888fdbba97e7ec55ddf52d6a1979bac7a53cf3c928a5d60a095b0365cd23"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.5/delphi-v0.5.5-x86_64-apple-darwin.tar.gz"
      sha256 "6ced99bc450e2de29a87b18eec14b6589478439e75490dfd68f79639fdcdf78b"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.5/delphi-v0.5.5-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "9f1745be302873dde578d8a8148732ff441f927b0662d821cc5116e944b97835"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.5/delphi-v0.5.5-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "734172512f38a34230332f08796d325598242beeb210d87769da518e70acdc88"
    end
  end

  def install
    bin.install "delphi", "delphi-ui"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
