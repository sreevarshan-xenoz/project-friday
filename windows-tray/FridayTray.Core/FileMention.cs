namespace FridayTray.Core;

using System.Text;
using System.Text.RegularExpressions;

public enum FileKind
{
    Text,
    Document,
    Picture,
    KeyOrPassword,
}

public static partial class FileMention
{
    public const int MaxContextChars = 8000;

    [GeneratedRegex(@"(?:^|\s)@file(?:\s+|$)", RegexOptions.IgnoreCase)]
    private static partial Regex AnyFileMention();

    [GeneratedRegex(@"(?:^|\s)@file\s+""[^""]*$", RegexOptions.IgnoreCase)]
    private static partial Regex UnclosedQuotePattern();

    [GeneratedRegex(@"(^|\s)@file\s+(""([^""]+)""|([^\s""]+))(?=\s|$)", RegexOptions.IgnoreCase)]
    private static partial Regex FileMentionPattern();

    public static (string Question, string? Path, string? Error) Parse(string question)
    {
        if (string.IsNullOrWhiteSpace(question))
            return (question ?? "", null, null);

        var matches = FileMentionPattern().Matches(question);
        var anyMatches = AnyFileMention().Matches(question);

        if (anyMatches.Count > 1 || matches.Count > 1)
            return (question, null, "One @file at a time.");

        if (matches.Count == 1)
        {
            var match = matches[0];
            var quoted = match.Groups[3].Success;
            var path = quoted ? match.Groups[3].Value : match.Groups[4].Value;

            // Remove the @file mention token and normalize spacing
            var prefixSpace = match.Groups[1].Value.Length > 0 ? " " : "";
            var cleaned = question.Remove(match.Index, match.Length);
            if (prefixSpace.Length > 0 && match.Index > 0 && match.Index < cleaned.Length)
                cleaned = cleaned.Insert(match.Index, " ");

            cleaned = Regex.Replace(cleaned, @"\s+", " ").Trim();
            return (cleaned, path, null);
        }

        if (UnclosedQuotePattern().IsMatch(question))
            return (question, null, "@file \"… needs its closing quote.");

        return (question, null, null);
    }

    public static bool IsPicture(string path)
    {
        var ext = Path.GetExtension(path);
        return ext.Equals(".png", StringComparison.OrdinalIgnoreCase) ||
               ext.Equals(".jpg", StringComparison.OrdinalIgnoreCase) ||
               ext.Equals(".jpeg", StringComparison.OrdinalIgnoreCase) ||
               ext.Equals(".webp", StringComparison.OrdinalIgnoreCase) ||
               ext.Equals(".gif", StringComparison.OrdinalIgnoreCase);
    }

    public static bool IsDocument(string path)
    {
        var ext = Path.GetExtension(path);
        return ext.Equals(".docx", StringComparison.OrdinalIgnoreCase) ||
               ext.Equals(".pptx", StringComparison.OrdinalIgnoreCase) ||
               ext.Equals(".xlsx", StringComparison.OrdinalIgnoreCase) ||
               ext.Equals(".odt", StringComparison.OrdinalIgnoreCase) ||
               ext.Equals(".odp", StringComparison.OrdinalIgnoreCase) ||
               ext.Equals(".ods", StringComparison.OrdinalIgnoreCase) ||
               ext.Equals(".rtf", StringComparison.OrdinalIgnoreCase) ||
               ext.Equals(".epub", StringComparison.OrdinalIgnoreCase) ||
               ext.Equals(".pdf", StringComparison.OrdinalIgnoreCase);
    }

    public static bool IsKeyOrPasswordFile(string path)
    {
        var name = Path.GetFileName(path);
        if (string.IsNullOrEmpty(name))
            return false;

        // .env, .env.*
        if (name.Equals(".env", StringComparison.OrdinalIgnoreCase) ||
            name.StartsWith(".env.", StringComparison.OrdinalIgnoreCase))
            return true;

        // *.pem, *.key, *.kdbx
        if (name.EndsWith(".pem", StringComparison.OrdinalIgnoreCase) ||
            name.EndsWith(".key", StringComparison.OrdinalIgnoreCase) ||
            name.EndsWith(".kdbx", StringComparison.OrdinalIgnoreCase))
            return true;

        // id_rsa*, id_ed25519*, id_ecdsa*
        if (name.StartsWith("id_rsa", StringComparison.OrdinalIgnoreCase) ||
            name.StartsWith("id_ed25519", StringComparison.OrdinalIgnoreCase) ||
            name.StartsWith("id_ecdsa", StringComparison.OrdinalIgnoreCase))
            return true;

        // *credentials*
        if (name.Contains("credentials", StringComparison.OrdinalIgnoreCase))
            return true;

        // .netrc, .pgpass
        if (name.Equals(".netrc", StringComparison.OrdinalIgnoreCase) ||
            name.Equals(".pgpass", StringComparison.OrdinalIgnoreCase))
            return true;

        return false;
    }

    public static FileKind Classify(string path)
    {
        if (IsPicture(path))
            return FileKind.Picture;
        if (IsKeyOrPasswordFile(path))
            return FileKind.KeyOrPassword;
        if (IsDocument(path))
            return FileKind.Document;
        return FileKind.Text;
    }

    public static string ExpandTilde(string path, string? userProfile = null)
    {
        if (string.IsNullOrEmpty(path))
            return path;

        var home = userProfile ?? Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        if (path == "~")
            return home;
        if (path.StartsWith("~/") || path.StartsWith("~\\"))
            return Path.Combine(home, path[2..]);

        return path;
    }

    public static (string? Text, string? Error) ReadTextFile(string path, int maxChars = MaxContextChars)
    {
        if (!File.Exists(path))
            return (null, "@file needs a text file, a PDF or a document (Word, PowerPoint, Excel, OpenDocument, RTF, EPUB) I can read.");

        byte[] buffer = new byte[8192];
        int bytesRead;
        try
        {
            using var fs = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);
            bytesRead = fs.Read(buffer, 0, buffer.Length);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return (null, "@file needs a text file, a PDF or a document (Word, PowerPoint, Excel, OpenDocument, RTF, EPUB) I can read.");
        }

        for (int i = 0; i < bytesRead; i++)
        {
            if (buffer[i] == 0)
                return (null, "@file needs a text file, a PDF or a document (Word, PowerPoint, Excel, OpenDocument, RTF, EPUB) I can read.");
        }

        string text;
        try
        {
            using var reader = new StreamReader(path, Encoding.UTF8, true);
            char[] chars = new char[maxChars];
            int readChars = reader.ReadBlock(chars, 0, chars.Length);
            text = new string(chars, 0, readChars);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return (null, "@file needs a text file, a PDF or a document (Word, PowerPoint, Excel, OpenDocument, RTF, EPUB) I can read.");
        }

        if (string.IsNullOrWhiteSpace(text))
            return (null, "That file has no text I can read (a scanned PDF has only pictures; a document may be damaged or locked).");

        if (Asking.LooksLikeSecret(text))
            return (null, "That file looks like a password or token, so I won't send it.");

        return (text, null);
    }

    public static (string? Text, string? Error) ValidateDocumentText(string? text, int maxChars = MaxContextChars)
    {
        if (string.IsNullOrWhiteSpace(text))
            return (null, "That file has no text I can read (a scanned PDF has only pictures; a document may be damaged or locked).");

        var trimmed = text.Length > maxChars ? text[..maxChars] : text;

        if (Asking.LooksLikeSecret(trimmed))
            return (null, "That file looks like a password or token, so I won't send it.");

        return (trimmed, null);
    }
}
