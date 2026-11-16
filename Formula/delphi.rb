class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.6.3"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.3/delphi-v0.6.3-aarch64-apple-darwin.tar.gz"
      sha256 "d934e61486d54947067ec79705468c6e2b5c62062703f85b703972485fa9c8e0"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.3/delphi-v0.6.3-x86_64-apple-darwin.tar.gz"
      sha256 "d1ccb931f4af78018ef88a9787ba988375d1daf937d38e38f287c0faa7a91654"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.3/delphi-v0.6.3-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "6d7d3df0cebb995d371baa387042e8e42a8c69eebe998566d637afee1ef96d16"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.3/delphi-v0.6.3-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "a4b489d8c0bb6dbf24d255a1d987e94e9396179014c51019a447e92436d0882e"
    end
  end

  def install
    bin.install "delphi", "delphi-ui"
  end

  def caveats
    <<~EOS
      The Delphi plugin for Claude Code and Copilot CLI runs ~/.local/bin/delphi.
      Link it once (`delphi connect claude-code` offers to do this for you):
        mkdir -p ~/.local/bin && ln -s #{HOMEBREW_PREFIX}/bin/delphi ~/.local/bin/delphi
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
