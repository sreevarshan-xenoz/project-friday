namespace FridayTray.Tests;

using System.Text;
using FridayTray.Core;
using Xunit;

public class FileMentionTests
{
    [Fact]
    public void Parses_plain_file_mention()
    {
        var (question, path, error) = FileMention.Parse("summarize @file notes.txt please");
        Assert.Null(error);
        Assert.Equal("notes.txt", path);
        Assert.Equal("summarize please", question);
    }

    [Fact]
    public void Parses_file_mention_at_start_and_end()
    {
        var (q1, p1, e1) = FileMention.Parse("@file notes.txt summarize this");
        Assert.Null(e1);
        Assert.Equal("notes.txt", p1);
        Assert.Equal("summarize this", q1);

        var (q2, p2, e2) = FileMention.Parse("summarize this @file notes.txt");
        Assert.Null(e2);
        Assert.Equal("notes.txt", p2);
        Assert.Equal("summarize this", q2);
    }

    [Fact]
    public void Parses_quoted_file_mention()
    {
        var (question, path, error) = FileMention.Parse(@"summarize @file ""My Documents\annual report.docx"" please");
        Assert.Null(error);
        Assert.Equal(@"My Documents\annual report.docx", path);
        Assert.Equal("summarize please", question);
    }

    [Fact]
    public void Refuses_unclosed_quote()
    {
        var (question, path, error) = FileMention.Parse(@"summarize @file ""My Documents\annual report.docx please");
        Assert.Null(path);
        Assert.Equal("@file \"… needs its closing quote.", error);
    }

    [Fact]
    public void Refuses_multiple_file_mentions()
    {
        var (q1, p1, e1) = FileMention.Parse("compare @file a.txt and @file b.txt");
        Assert.Null(p1);
        Assert.Equal("One @file at a time.", e1);

        var (q2, p2, e2) = FileMention.Parse(@"compare @file ""a.txt"" and @file ""b.txt""");
        Assert.Null(p2);
        Assert.Equal("One @file at a time.", e2);
    }

    [Fact]
    public void Returns_null_path_and_error_when_no_mention()
    {
        var (question, path, error) = FileMention.Parse("what is the weather today?");
        Assert.Null(error);
        Assert.Null(path);
        Assert.Equal("what is the weather today?", question);
    }

    [Fact]
    public void Ignores_email_addresses_or_tokens_not_starting_with_file_mention()
    {
        var (question, path, error) = FileMention.Parse("send email to test@file.com right now");
        Assert.Null(error);
        Assert.Null(path);
        Assert.Equal("send email to test@file.com right now", question);
    }

    [Theory]
    [InlineData("image.png", FileKind.Picture)]
    [InlineData("photo.jpg", FileKind.Picture)]
    [InlineData("pic.jpeg", FileKind.Picture)]
    [InlineData("shot.webp", FileKind.Picture)]
    [InlineData("anim.gif", FileKind.Picture)]
    [InlineData("report.docx", FileKind.Document)]
    [InlineData("slides.pptx", FileKind.Document)]
    [InlineData("sheet.xlsx", FileKind.Document)]
    [InlineData("doc.odt", FileKind.Document)]
    [InlineData("doc.odp", FileKind.Document)]
    [InlineData("doc.ods", FileKind.Document)]
    [InlineData("doc.rtf", FileKind.Document)]
    [InlineData("book.epub", FileKind.Document)]
    [InlineData("paper.pdf", FileKind.Document)]
    [InlineData(".env", FileKind.KeyOrPassword)]
    [InlineData(".env.production", FileKind.KeyOrPassword)]
    [InlineData("cert.pem", FileKind.KeyOrPassword)]
    [InlineData("secret.key", FileKind.KeyOrPassword)]
    [InlineData("id_rsa", FileKind.KeyOrPassword)]
    [InlineData("id_rsa.pub", FileKind.KeyOrPassword)]
    [InlineData("id_ed25519", FileKind.KeyOrPassword)]
    [InlineData("id_ecdsa", FileKind.KeyOrPassword)]
    [InlineData("aws_credentials.json", FileKind.KeyOrPassword)]
    [InlineData("credentials", FileKind.KeyOrPassword)]
    [InlineData("database.kdbx", FileKind.KeyOrPassword)]
    [InlineData(".netrc", FileKind.KeyOrPassword)]
    [InlineData(".pgpass", FileKind.KeyOrPassword)]
    [InlineData("notes.txt", FileKind.Text)]
    [InlineData("script.py", FileKind.Text)]
    [InlineData("main.rs", FileKind.Text)]
    public void Classifies_file_kind_correctly(string filename, FileKind expected)
    {
        Assert.Equal(expected, FileMention.Classify(filename));
    }

    [Theory]
    [InlineData(".env", true)]
    [InlineData(".env.local", true)]
    [InlineData(".env.prod.backup", true)]
    [InlineData("cert.pem", true)]
    [InlineData("mykey.key", true)]
    [InlineData("id_rsa", true)]
    [InlineData("id_rsa_corp", true)]
    [InlineData("id_ed25519_github", true)]
    [InlineData("id_ecdsa_test", true)]
    [InlineData("my_credentials.ini", true)]
    [InlineData("safe.kdbx", true)]
    [InlineData(".netrc", true)]
    [InlineData(".pgpass", true)]
    [InlineData("notes.env.txt", false)]
    [InlineData("normal.txt", false)]
    [InlineData("document.pdf", false)]
    public void Identifies_key_and_password_files(string filename, bool expected)
    {
        Assert.Equal(expected, FileMention.IsKeyOrPasswordFile(filename));
    }

    [Fact]
    public void BuildRequest_uses_8000_context_cap_when_specified_and_4000_default()
    {
        var longText = new string('A', 10000);

        // Default cap is 4000
        var defaultReq = Asking.BuildRequest("Summarize this", longText);
        Assert.Contains(new string('A', 4000), defaultReq);
        Assert.DoesNotContain(new string('A', 4001), defaultReq);

        // Explicit 8000 cap for file mentions
        var fileReq = Asking.BuildRequest("Summarize this", longText, 8000);
        Assert.Contains(new string('A', 8000), fileReq);
        Assert.DoesNotContain(new string('A', 8001), fileReq);

        // Shorter text is not truncated
        var shortText = new string('B', 6000);
        var shortReq = Asking.BuildRequest("Explain", shortText, 8000);
        Assert.Contains(shortText, shortReq);
    }

    [Fact]
    public void ReadTextFile_rejects_binary_files_with_nul_bytes()
    {
        var tempFile = Path.GetTempFileName();
        try
        {
            var bytes = new byte[] { 0x48, 0x65, 0x6C, 0x6C, 0x6F, 0x00, 0x57, 0x6F, 0x72, 0x6C, 0x64 };
            File.WriteAllBytes(tempFile, bytes);

            var (text, error) = FileMention.ReadTextFile(tempFile);
            Assert.Null(text);
            Assert.Equal("@file needs a text file, a PDF or a document (Word, PowerPoint, Excel, OpenDocument, RTF, EPUB) I can read.", error);
        }
        finally
        {
            File.Delete(tempFile);
        }
    }

    [Fact]
    public void ReadTextFile_rejects_empty_file()
    {
        var tempFile = Path.GetTempFileName();
        try
        {
            File.WriteAllText(tempFile, "   \n\t   ");

            var (text, error) = FileMention.ReadTextFile(tempFile);
            Assert.Null(text);
            Assert.Equal("That file has no text I can read (a scanned PDF has only pictures; a document may be damaged or locked).", error);
        }
        finally
        {
            File.Delete(tempFile);
        }
    }

    [Fact]
    public void ReadTextFile_rejects_secret_shaped_content()
    {
        var tempFile = Path.GetTempFileName();
        try
        {
            // A long mixed token that LooksLikeSecret flags
            File.WriteAllText(tempFile, "ghp_1234567890abcdefghijklmnopqrstuvwxyzABCD");

            var (text, error) = FileMention.ReadTextFile(tempFile);
            Assert.Null(text);
            Assert.Equal("That file looks like a password or token, so I won't send it.", error);
        }
        finally
        {
            File.Delete(tempFile);
        }
    }

    [Fact]
    public void ReadTextFile_reads_valid_text_up_to_max_chars()
    {
        var tempFile = Path.GetTempFileName();
        try
        {
            var content = new string('x', 9000);
            File.WriteAllText(tempFile, content);

            var (text, error) = FileMention.ReadTextFile(tempFile, 8000);
            Assert.Null(error);
            Assert.NotNull(text);
            Assert.Equal(8000, text.Length);
        }
        finally
        {
            File.Delete(tempFile);
        }
    }

    [Fact]
    public void ValidateDocumentText_handles_empty_secret_and_cap()
    {
        var (emptyText, emptyErr) = FileMention.ValidateDocumentText("   ");
        Assert.Null(emptyText);
        Assert.Equal("That file has no text I can read (a scanned PDF has only pictures; a document may be damaged or locked).", emptyErr);

        var (secretText, secretErr) = FileMention.ValidateDocumentText("ghp_1234567890abcdefghijklmnopqrstuvwxyzABCD");
        Assert.Null(secretText);
        Assert.Equal("That file looks like a password or token, so I won't send it.", secretErr);

        var (validText, validErr) = FileMention.ValidateDocumentText(new string('c', 9000), 8000);
        Assert.Null(validErr);
        Assert.NotNull(validText);
        Assert.Equal(8000, validText.Length);
    }

    [Fact]
    public void ExpandTilde_expands_user_home()
    {
        var home = @"C:\Users\TestUser";
        Assert.Equal(@"C:\Users\TestUser", FileMention.ExpandTilde("~", home));
        Assert.Equal(Path.Combine(home, "notes.txt"), FileMention.ExpandTilde("~/notes.txt", home));
        Assert.Equal(Path.Combine(home, "notes.txt"), FileMention.ExpandTilde(@"~\notes.txt", home));
        Assert.Equal(@"C:\Other\file.txt", FileMention.ExpandTilde(@"C:\Other\file.txt", home));
    }
}
