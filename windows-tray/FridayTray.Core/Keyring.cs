using System.Text.RegularExpressions;

namespace FridayTray.Core;

/// <summary>
/// Secrets (API keys, tokens) live in the OS keychain, not friday.env: the file
/// keeps `NAME=keyring:` and the value sits in Windows Credential Manager under
/// `friday/NAME`. These functions take the keychain as delegates so they test
/// anywhere; the tray passes its CredentialStore.
/// </summary>
public static class Keyring
{
    /// <summary>What a secret's line holds in friday.env instead of the value.</summary>
    public const string Marker = "keyring:";

    private static readonly Regex SecretName = new("^[A-Z0-9_]+_(API_KEY|TOKEN|SECRET)$", RegexOptions.Compiled);

    public static bool IsSecret(string name) => SecretName.IsMatch(name);

    /// <summary>
    /// A copy of <paramref name="env"/> with each marker replaced by its value
    /// from the keychain. A credential that is gone drops the variable: the
    /// marker itself is never passed on.
    /// </summary>
    public static IDictionary<string, string> Resolve(IReadOnlyDictionary<string, string> env, Func<string, string?> read)
    {
        var result = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var (name, value) in env)
        {
            if (value != Marker)
                result[name] = value;
            else if (read(name) is { Length: > 0 } secret)
                result[name] = secret;
        }
        return result;
    }

    /// <summary>
    /// Moves every secret with a real value into the keychain and leaves
    /// `NAME=keyring:` in its place. A line whose write fails stays as it was.
    /// Every other line, comment and the order are kept.
    /// </summary>
    public static string Migrate(string envText, Func<string, string, bool> write)
    {
        var lines = envText.Split('\n');
        for (var i = 0; i < lines.Length; i++)
        {
            var line = lines[i].TrimEnd('\r');
            var t = line.Trim();
            if (t.Length == 0 || t.StartsWith('#'))
                continue;
            var eq = t.IndexOf('=');
            if (eq <= 0)
                continue;
            var name = t[..eq].Trim();
            if (!IsSecret(name))
                continue;
            var value = EnvFile.Parse(t).GetValueOrDefault(name, "");
            if (value.Length == 0 || value == Marker)
                continue;
            if (write(name, value))
                lines[i] = $"{name}={Marker}" + lines[i][line.Length..];
        }
        return string.Join('\n', lines);
    }

    /// <summary>
    /// The file after setting <paramref name="name"/>. A secret goes to the
    /// keychain and the file gets the marker; if the keychain refuses, the plain
    /// value is kept in the file as before. An empty value clears both.
    /// </summary>
    public static string Store(string envText, string name, string? value, Func<string, string, bool> write, Action<string> delete)
    {
        if (string.IsNullOrEmpty(value))
        {
            delete(name);
            return EnvFile.WithSetting(envText, name, null);
        }
        return EnvFile.WithSetting(envText, name, write(name, value) ? Marker : value);
    }
}
