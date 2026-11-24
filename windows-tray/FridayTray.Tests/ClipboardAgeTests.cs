using FridayTray.Core;
using Xunit;

public class ClipboardAgeTests
{
    [Fact]
    public void An_old_copy_is_not_used_and_a_fresh_or_unknown_one_is()
    {
        var ages = new ClipboardAge();
        var t0 = new DateTime(2026, 11, 24, 9, 0, 0, DateTimeKind.Utc);
        ages.Seen("old text", t0);
        ages.Seen("new text", t0.AddMinutes(20));
        // Put back after a selection was read: it keeps its first time.
        ages.Seen("old text", t0.AddMinutes(21));
        var now = t0.AddMinutes(22);
        Assert.Contains("copied 22 minutes ago", ages.TooOld("old text", now));
        Assert.Null(ages.TooOld("new text", now));
        Assert.Null(ages.TooOld("never seen", now));
        Assert.Null(ages.Age("never seen", now));
    }

    [Fact]
    public void Only_the_newest_fifty_are_kept()
    {
        var ages = new ClipboardAge();
        var t0 = DateTime.UtcNow;
        for (var i = 0; i < 60; i++)
            ages.Seen($"clip {i}", t0);
        Assert.Null(ages.Age("clip 0", t0));
        Assert.NotNull(ages.Age("clip 59", t0));
    }
}
