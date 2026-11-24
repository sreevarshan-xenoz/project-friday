namespace FridayTray.Core;

using System.Text.RegularExpressions;

/// <summary>What a line of an answer is.</summary>
public enum BlockKind { Paragraph, Heading, Bullet, Numbered, Code, Table }

/// <summary>A run of text with its marks.</summary>
public sealed record Span(string Text, bool Bold = false, bool Italic = false, bool Code = false);

/// <summary>One block of an answer; <see cref="Marker"/> is a numbered item's
/// number; <see cref="Rows"/> a table's cells, its header row first.</summary>
public sealed record Block(BlockKind Kind, IReadOnlyList<Span> Spans, string Marker = "", IReadOnlyList<IReadOnlyList<string>>? Rows = null);

/// <summary>
/// An answer's Markdown as blocks and marked spans, for the answer window to
/// draw: bold, italic, inline code, code blocks, headings, bullet and numbered
/// lists, and links (their text). Anything else stays as written.
/// </summary>
public static partial class MarkdownBlocks
{
    public static IReadOnlyList<Block> Parse(string markdown)
    {
        var blocks = new List<Block>();
        List<string>? code = null;
        List<IReadOnlyList<string>>? table = null;
        void EndTable()
        {
            if (table is { Count: > 0 })
                blocks.Add(new Block(BlockKind.Table, [], Rows: table));
            table = null;
        }
        foreach (var raw in markdown.Replace("\r\n", "\n").Split('\n'))
        {
            var line = raw.TrimEnd();
            // A table: rows of | cells |; the --- row under the header is skipped.
            if (code is null && line.TrimStart().StartsWith('|') && line.Trim().Length > 1)
            {
                var cells = line.Trim().Trim('|').Split('|').Select(c => c.Trim()).ToList();
                if (!cells.All(c => c.Length > 0 && c.Trim(':', '-', ' ').Length == 0))
                    (table ??= []).Add(cells);
                continue;
            }
            EndTable();
            if (line.TrimStart().StartsWith("```"))
            {
                if (code is null)
                    code = [];
                else
                {
                    blocks.Add(new Block(BlockKind.Code, [new Span(string.Join("\n", code), Code: true)]));
                    code = null;
                }
                continue;
            }
            if (code is not null)
            {
                code.Add(raw);
                continue;
            }
            if (line.Length == 0)
                continue;
            if (HeadingRx().Match(line) is { Success: true } h)
                blocks.Add(new Block(BlockKind.Heading, Inline(h.Groups[1].Value)));
            else if (BulletRx().Match(line) is { Success: true } b)
                blocks.Add(new Block(BlockKind.Bullet, Inline(b.Groups[2].Value)));
            else if (NumberedRx().Match(line) is { Success: true } n)
                blocks.Add(new Block(BlockKind.Numbered, Inline(n.Groups[3].Value), n.Groups[2].Value));
            else
                blocks.Add(new Block(BlockKind.Paragraph, Inline(line)));
        }
        EndTable();
        // An unclosed fence still shows its code.
        if (code is { Count: > 0 })
            blocks.Add(new Block(BlockKind.Code, [new Span(string.Join("\n", code), Code: true)]));
        return blocks;
    }

    /// <summary>The spans of one line: the earliest mark wins, and marks nest (bold inside a link, italic inside bold).</summary>
    public static IReadOnlyList<Span> Inline(string s, bool bold = false, bool italic = false)
    {
        var spans = new List<Span>();
        var at = 0;
        while (at < s.Length)
        {
            Match? first = null;
            var kind = 0;
            foreach (var (rx, k) in new[] { (CodeRx(), 1), (LinkRx(), 2), (BoldRx(), 3), (ItalicRx(), 4) })
            {
                var m = rx.Match(s, at);
                if (m.Success && (first is null || m.Index < first.Index))
                    (first, kind) = (m, k);
            }
            if (first is null)
                break;
            if (first.Index > at)
                spans.Add(new Span(s[at..first.Index], bold, italic));
            switch (kind)
            {
                case 1: spans.Add(new Span(first.Groups[1].Value, bold, italic, Code: true)); break;
                case 2: spans.AddRange(Inline(first.Groups[1].Value, bold, italic)); break;
                case 3: spans.AddRange(Inline(first.Groups[2].Value, true, italic)); break;
                default: spans.AddRange(Inline(first.Groups[2].Value, bold, true)); break;
            }
            at = first.Index + first.Length;
        }
        if (at < s.Length)
            spans.Add(new Span(s[at..], bold, italic));
        return spans;
    }

    [GeneratedRegex(@"^\s{0,3}#{1,6}\s+(.*)$")] private static partial Regex HeadingRx();
    [GeneratedRegex(@"^(\s*)[-*+]\s+(.*)$")] private static partial Regex BulletRx();
    [GeneratedRegex(@"^(\s*)(\d+)[.)]\s+(.*)$")] private static partial Regex NumberedRx();
    [GeneratedRegex(@"\[([^\]]+)\]\([^)\s]+\)")] private static partial Regex LinkRx();
    [GeneratedRegex(@"`([^`]+)`")] private static partial Regex CodeRx();
    [GeneratedRegex(@"(\*\*|__)(?=\S)(.+?)(?<=\S)\1")] private static partial Regex BoldRx();
    [GeneratedRegex(@"(?<![\w*])(\*|_)(?=\S)(.+?)(?<=\S)\1(?![\w*])")] private static partial Regex ItalicRx();
}
