class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.6.4"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.4/delphi-v0.6.4-aarch64-apple-darwin.tar.gz"
      sha256 "646b8171ee2516529255880c32891bbbbbd01ce0ea83e6610537845419004a18"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.4/delphi-v0.6.4-x86_64-apple-darwin.tar.gz"
      sha256 "ae6b3c60ace66b497ff0d9c665699682859c72a65fa1e1399bbe795aa130bbff"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.4/delphi-v0.6.4-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "67ecf731ffddd5518186130de3452ecaee3ac538ef27c7a62c4fa120716ebb32"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.4/delphi-v0.6.4-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "021f94b4a928d8a7bd3bb5d5dcd7e3a17570cd41ead463870ef3d50cbe60da68"
    end
  end

  def install
    bin.install "delphi", "delphi-ui"
    # The desktop app (specs/122), on macOS: kept in the prefix; the caveats say how to link it.
    prefix.install "Delphi.app" if OS.mac? && File.directory?("Delphi.app")
  end

  def caveats
    s = <<~EOS
      The Delphi plugin for Claude Code and Copilot CLI runs ~/.local/bin/delphi.
      Link it once (`delphi connect claude-code` offers to do this for you):
        mkdir -p ~/.local/bin && ln -s #{HOMEBREW_PREFIX}/bin/delphi ~/.local/bin/delphi
    EOS
    if OS.mac?
      s += <<~EOS
        The desktop app is in #{opt_prefix}/Delphi.app. Link it where `delphi app` looks:
          mkdir -p ~/Applications && ln -sfn #{opt_prefix}/Delphi.app ~/Applications/Delphi.app
      EOS
    end
    s
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
