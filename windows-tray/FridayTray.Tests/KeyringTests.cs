using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public class KeyringTests
{
    [Theory]
    [InlineData("OPENCODE_API_KEY", true)]
    [InlineData("GITHUB_TOKEN", true)]
    [InlineData("CLIENT_SECRET", true)]
    [InlineData("FRIDAY_AUTONOMY", false)]
    [InlineData("openai_api_key", false)]
    [InlineData("API_KEY", false)]
    [InlineData("MY_API_KEY_PATH", false)]
    public void IsSecret_matches_key_token_and_secret_names(string name, bool expected) =>
        Assert.Equal(expected, Keyring.IsSecret(name));

    [Fact]
    public void Resolve_replaces_markers_drops_missing_and_leaves_the_rest()
    {
        var env = new Dictionary<string, string>
        {
            ["GEMINI_API_KEY"] = Keyring.Marker,
            ["GROQ_API_KEY"] = Keyring.Marker,
            ["OLD_API_KEY"] = "plain-value",
            ["FRIDAY_AUTONOMY"] = "act",
        };
        var result = Keyring.Resolve(env, n => n == "GEMINI_API_KEY" ? "secret" : null);
        Assert.Equal("secret", result["GEMINI_API_KEY"]);
        Assert.False(result.ContainsKey("GROQ_API_KEY"));
        Assert.Equal("plain-value", result["OLD_API_KEY"]);
        Assert.Equal("act", result["FRIDAY_AUTONOMY"]);
    }

    [Fact]
    public void Migrate_moves_only_secrets_with_real_values_and_keeps_everything_else()
    {
        var text = "# my keys\nGEMINI_API_KEY=abc123\nFRIDAY_AUTONOMY=act\nGROQ_API_KEY=\nCEREBRAS_API_KEY=keyring:\nGITHUB_TOKEN=\"tok\"\n";
        var written = new Dictionary<string, string>();
        var result = Keyring.Migrate(text, (n, v) => { written[n] = v; return true; });
        Assert.Equal("# my keys\nGEMINI_API_KEY=keyring:\nFRIDAY_AUTONOMY=act\nGROQ_API_KEY=\nCEREBRAS_API_KEY=keyring:\nGITHUB_TOKEN=keyring:\n", result);
        Assert.Equal(new Dictionary<string, string> { ["GEMINI_API_KEY"] = "abc123", ["GITHUB_TOKEN"] = "tok" }, written);
    }

    [Fact]
    public void Migrate_keeps_the_plain_line_when_the_write_fails()
    {
        var text = "GEMINI_API_KEY=abc\nGROQ_API_KEY=def\n";
        var result = Keyring.Migrate(text, (n, _) => n == "GROQ_API_KEY");
        Assert.Equal("GEMINI_API_KEY=abc\nGROQ_API_KEY=keyring:\n", result);
    }

    [Fact]
    public void Migrate_is_idempotent_and_keeps_windows_line_endings()
    {
        var calls = 0;
        var once = Keyring.Migrate("GEMINI_API_KEY=abc\r\nX=1\r\n", (_, _) => { calls++; return true; });
        Assert.Equal("GEMINI_API_KEY=keyring:\r\nX=1\r\n", once);
        Assert.Equal(once, Keyring.Migrate(once, (_, _) => { calls++; return true; }));
        Assert.Equal(1, calls);
    }

    [Fact]
    public void Store_writes_the_secret_and_puts_the_marker_in_the_file()
    {
        string? stored = null;
        var result = Keyring.Store("A=1\n", "GEMINI_API_KEY", "abc", (_, v) => { stored = v; return true; }, _ => { });
        Assert.Equal("abc", stored);
        Assert.Equal("A=1\nGEMINI_API_KEY=keyring:\n", result);
    }

    [Fact]
    public void Store_falls_back_to_the_plain_value_when_the_write_fails()
    {
        var result = Keyring.Store("A=1\n", "GEMINI_API_KEY", "abc", (_, _) => false, _ => { });
        Assert.Equal("A=1\nGEMINI_API_KEY=abc\n", result);
    }

    [Fact]
    public void Store_with_no_value_deletes_the_credential_and_the_line()
    {
        string? deleted = null;
        var result = Keyring.Store("A=1\nGEMINI_API_KEY=keyring:\n", "GEMINI_API_KEY", null, (_, _) => true, n => deleted = n);
        Assert.Equal("GEMINI_API_KEY", deleted);
        Assert.Equal("A=1\n", result);
    }
}
