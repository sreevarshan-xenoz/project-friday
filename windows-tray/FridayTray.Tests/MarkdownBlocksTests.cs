using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public class MarkdownBlocksTests
{
    private static string Plain(IEnumerable<Span> spans) => string.Concat(spans.Select(s => s.Text));

    [Fact]
    public void Bold_italic_code_and_links_become_marks_not_asterisks()
    {
        var spans = MarkdownBlocks.Inline("Mostly **photos** and *cache*, see `du -sh` or [the docs](https://example.com).");
        Assert.Equal("Mostly photos and cache, see du -sh or the docs.", Plain(spans));
        Assert.Contains(spans, s => s.Text == "photos" && s.Bold && !s.Italic);
        Assert.Contains(spans, s => s.Text == "cache" && s.Italic && !s.Bold);
        Assert.Contains(spans, s => s.Text == "du -sh" && s.Code);
        Assert.Contains(spans, s => s.Text == "the docs" && !s.Bold && !s.Code);
    }

    [Fact]
    public void Marks_nest_inside_each_other()
    {
        var spans = MarkdownBlocks.Inline("**bold with *italic* inside** and [**a bold link**](https://example.com)");
        Assert.Contains(spans, s => s.Text == "italic" && s.Bold && s.Italic);
        Assert.Contains(spans, s => s.Text == "a bold link" && s.Bold);
    }

    [Fact]
    public void Lists_headings_and_code_blocks()
    {
        var blocks = MarkdownBlocks.Parse("# Disk\n\n- Pictures: 41 GB\n2. Empty **Downloads**\n```\nrm -rf ~/Downloads/*\nls\n```\nDone.");
        Assert.Equal(new[] { BlockKind.Heading, BlockKind.Bullet, BlockKind.Numbered, BlockKind.Code, BlockKind.Paragraph },
            blocks.Select(b => b.Kind));
        Assert.Equal("Disk", Plain(blocks[0].Spans));
        Assert.Equal("2", blocks[2].Marker);
        Assert.Contains(blocks[2].Spans, s => s.Text == "Downloads" && s.Bold);
        Assert.Equal("rm -rf ~/Downloads/*\nls", blocks[3].Spans.Single().Text);
    }

    [Fact]
    public void Underscores_inside_words_and_arithmetic_stay_as_written()
    {
        Assert.Equal("my_file_name.txt and 2 * 3 * 4", Plain(MarkdownBlocks.Inline("my_file_name.txt and 2 * 3 * 4")));
        Assert.All(MarkdownBlocks.Inline("my_file_name.txt and 2 * 3 * 4"), s => Assert.False(s.Italic));
    }

    [Fact]
    public void An_unclosed_fence_still_shows_its_code_and_blank_lines_vanish()
    {
        var blocks = MarkdownBlocks.Parse("Run:\n\n\n```\nfriday doctor");
        Assert.Equal(new[] { BlockKind.Paragraph, BlockKind.Code }, blocks.Select(b => b.Kind));
        Assert.Equal("friday doctor", blocks[1].Spans.Single().Text);
    }
}
