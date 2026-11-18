namespace FridayTray.Core;

using System.Text;
using System.Text.RegularExpressions;

/// <summary>
/// Turns an answer's Markdown into RTF for the answer window, so "**photos**"
/// reads as bold photos, not asterisks: bold, italic, inline code, code
/// blocks, headings, bullet and numbered lists, and links (their text).
/// Anything else stays as written.
/// </summary>
public static partial class MarkdownRtf
{
    /// <param name="font">the body font family</param>
    /// <param name="sizePt">its size in points</param>
    /// <param name="text">body colour</param>
    /// <param name="code">code colour</param>
    public static string Convert(string markdown, string font, float sizePt, (int R, int G, int B) text, (int R, int G, int B) code)
    {
        var sb = new StringBuilder();
        sb.Append(@"{\rtf1\ansi\deff0{\fonttbl{\f0 ").Append(font).Append(@";}{\f1 Consolas;}}");
        sb.Append(@"{\colortbl;").Append(Color(text)).Append(Color(code)).Append('}');
        sb.Append(@"\f0\fs").Append((int)Math.Round(sizePt * 2)).Append(@"\cf1\sa60 ");
        var inCode = false;
        foreach (var raw in markdown.Replace("\r\n", "\n").Split('\n'))
        {
            var line = raw.TrimEnd();
            if (line.TrimStart().StartsWith("```"))
            {
                inCode = !inCode;
                continue;
            }
            if (inCode)
            {
                sb.Append(@"{\f1\cf2 ").Append(Escape(raw)).Append(@"}\par ");
                continue;
            }
            var heading = HeadingRx().Match(line);
            if (heading.Success)
            {
                sb.Append(@"{\b ").Append(Inline(heading.Groups[1].Value)).Append(@"}\par ");
                continue;
            }
            var bullet = BulletRx().Match(line);
            if (bullet.Success)
            {
                sb.Append(@"\li360\fi-200 \u8226? ").Append(Inline(bullet.Groups[2].Value)).Append(@"\par\li0\fi0 ");
                continue;
            }
            var numbered = NumberedRx().Match(line);
            if (numbered.Success)
            {
                sb.Append(@"\li360\fi-260 ").Append(Escape(numbered.Groups[2].Value)).Append(". ")
                  .Append(Inline(numbered.Groups[3].Value)).Append(@"\par\li0\fi0 ");
                continue;
            }
            sb.Append(Inline(line)).Append(@"\par ");
        }
        sb.Append('}');
        return sb.ToString();
    }

    private static string Color((int R, int G, int B) c) => $@"\red{c.R}\green{c.G}\blue{c.B};";

    /// <summary>Plain text made safe for RTF: braces, backslashes, and anything outside ASCII.</summary>
    public static string Escape(string s)
    {
        var sb = new StringBuilder(s.Length);
        foreach (var ch in s)
        {
            if (ch is '\\' or '{' or '}') sb.Append('\\').Append(ch);
            else if (ch > 127) sb.Append(@"\u").Append((short)ch).Append('?');
            else if (ch == '\t') sb.Append(@"\tab ");
            else sb.Append(ch);
        }
        return sb.ToString();
    }

    // Inline marks, applied after escaping (none of * _ ` [ ] ( ) is escaped).
    private static string Inline(string s)
    {
        var t = Escape(s);
        t = LinkRx().Replace(t, "$1");
        t = CodeRx().Replace(t, @"{\f1\cf2 $1}");
        t = BoldRx().Replace(t, @"{\b $2}");
        t = ItalicRx().Replace(t, @"{\i $2}");
        return t;
    }

    [GeneratedRegex(@"^\s{0,3}#{1,6}\s+(.*)$")] private static partial Regex HeadingRx();
    [GeneratedRegex(@"^(\s*)[-*+]\s+(.*)$")] private static partial Regex BulletRx();
    [GeneratedRegex(@"^(\s*)(\d+)[.)]\s+(.*)$")] private static partial Regex NumberedRx();
    [GeneratedRegex(@"\[([^\]]+)\]\([^)\s]+\)")] private static partial Regex LinkRx();
    [GeneratedRegex(@"`([^`]+)`")] private static partial Regex CodeRx();
    [GeneratedRegex(@"(\*\*|__)(?=\S)(.+?)(?<=\S)\1")] private static partial Regex BoldRx();
    [GeneratedRegex(@"(?<![\w*])(\*|_)(?=\S)(.+?)(?<=\S)\1(?![\w*])")] private static partial Regex ItalicRx();
}
