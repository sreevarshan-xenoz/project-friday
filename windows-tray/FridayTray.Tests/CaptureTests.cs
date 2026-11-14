using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public class FocusWaitTests
{
    [Fact]
    public async Task Returns_true_immediately_when_already_equal()
    {
        var target = (IntPtr)0x1234;
        var polls = 0;
        var delayed = false;

        var result = await FocusWait.WaitForForegroundAsync(
            target,
            () => { polls++; return target; },
            delay: _ => { delayed = true; return Task.CompletedTask; },
            timeout: TimeSpan.FromMilliseconds(500));

        Assert.True(result);
        Assert.Equal(1, polls);
        Assert.False(delayed);
    }

    [Fact]
    public async Task Returns_true_after_several_polls_when_equal()
    {
        var target = (IntPtr)0x1234;
        var other = (IntPtr)0x5678;
        var polls = 0;
        var delayCount = 0;
        var now = new DateTime(2026, 1, 1, 12, 0, 0);

        var result = await FocusWait.WaitForForegroundAsync(
            target,
            () =>
            {
                polls++;
                return polls >= 3 ? target : other;
            },
            delay: d =>
            {
                delayCount++;
                now += d;
                return Task.CompletedTask;
            },
            getNow: () => now,
            timeout: TimeSpan.FromMilliseconds(500),
            pollInterval: TimeSpan.FromMilliseconds(25));

        Assert.True(result);
        Assert.Equal(3, polls);
        Assert.Equal(2, delayCount);
        Assert.Equal(new DateTime(2026, 1, 1, 12, 0, 0).AddMilliseconds(50), now);
    }

    [Fact]
    public async Task Times_out_and_returns_false_when_never_equal()
    {
        var target = (IntPtr)0x1234;
        var other = (IntPtr)0x9999;
        var delayCount = 0;
        var now = new DateTime(2026, 1, 1, 12, 0, 0);

        var result = await FocusWait.WaitForForegroundAsync(
            target,
            () => other,
            delay: d =>
            {
                delayCount++;
                now += d;
                return Task.CompletedTask;
            },
            getNow: () => now,
            timeout: TimeSpan.FromMilliseconds(500),
            pollInterval: TimeSpan.FromMilliseconds(25));

        Assert.False(result);
        Assert.Equal(20, delayCount);
        Assert.True(now >= new DateTime(2026, 1, 1, 12, 0, 0).AddMilliseconds(500));
    }

    [Fact]
    public async Task Returns_false_immediately_for_zero_target()
    {
        var polls = 0;
        var delayed = false;

        var result = await FocusWait.WaitForForegroundAsync(
            IntPtr.Zero,
            () => { polls++; return IntPtr.Zero; },
            delay: _ => { delayed = true; return Task.CompletedTask; });

        Assert.False(result);
        Assert.Equal(0, polls);
        Assert.False(delayed);
    }

    [Fact]
    public void Synchronous_wait_returns_when_equal_and_times_out()
    {
        var target = (IntPtr)0x100;
        var other = (IntPtr)0x200;
        var now = new DateTime(2026, 1, 1, 12, 0, 0);
        var polls = 0;

        // Equal after 2 polls
        var ok = FocusWait.WaitForForeground(
            target,
            () => ++polls >= 2 ? target : other,
            sleep: d => now += d,
            getNow: () => now,
            timeout: TimeSpan.FromMilliseconds(100),
            pollInterval: TimeSpan.FromMilliseconds(20));

        Assert.True(ok);
        Assert.Equal(2, polls);

        // Times out
        now = new DateTime(2026, 1, 1, 12, 0, 0);
        var timedOut = FocusWait.WaitForForeground(
            target,
            () => other,
            sleep: d => now += d,
            getNow: () => now,
            timeout: TimeSpan.FromMilliseconds(100),
            pollInterval: TimeSpan.FromMilliseconds(20));

        Assert.False(timedOut);
    }
}

public class TerminalClassTests
{
    [Theory]
    [InlineData("ConsoleWindowClass", true)]
    [InlineData("consolewindowclass", true)]
    [InlineData("CONSOLEWINDOWCLASS", true)]
    [InlineData("  ConsoleWindowClass  ", true)]
    [InlineData("CASCADIA_HOSTING_WINDOW_CLASS", true)]
    [InlineData("cascadia_hosting_window_class", true)]
    [InlineData("mintty", true)]
    [InlineData("Mintty", true)]
    [InlineData("MINTTY", true)]
    [InlineData("  mintty  ", true)]
    public void Terminal_window_classes_are_recognized(string className, bool expected)
    {
        Assert.Equal(expected, Terminals.IsTerminalClass(className));
        Assert.Equal(expected, Terminals.IsTerminal(className));
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData("Notepad")]
    [InlineData("Chrome_WidgetWin_1")]
    [InlineData("CabinetWClass")]
    [InlineData("WindowsTerminal")]
    [InlineData("mintty2")]
    public void Non_terminal_classes_are_not_recognized(string? className)
    {
        Assert.False(Terminals.IsTerminalClass(className));
        Assert.False(Terminals.IsTerminal(className));
    }
}
