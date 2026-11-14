using FridayTray.Core;
using Xunit;

namespace FridayTray.Tests;

public class HotkeyRegistrationTests
{
    private sealed record Choice(string Label);

    [Fact]
    public void Returns_false_when_choices_is_empty()
    {
        var registered = false;
        var (ok, chosen) = HotkeyRegistration.RegisterWithRetry(
            Array.Empty<Choice>(),
            _ => { registered = true; return true; },
            delay: _ => { });

        Assert.False(ok);
        Assert.Null(chosen);
        Assert.False(registered);
    }

    [Fact]
    public void Registers_primary_immediately_on_first_attempt_without_delay()
    {
        var primary = new Choice("Win+Y");
        var fallback = new Choice("Ctrl+Alt+Y");
        var delays = 0;
        var registerCalls = new List<string>();

        var (ok, chosen) = HotkeyRegistration.RegisterWithRetry(
            new[] { primary, fallback },
            c =>
            {
                registerCalls.Add(c.Label);
                return c == primary;
            },
            delay: _ => delays++);

        Assert.True(ok);
        Assert.Same(primary, chosen);
        Assert.Equal(0, delays);
        Assert.Equal(new[] { "Win+Y" }, registerCalls);
    }

    [Fact]
    public void Retries_primary_and_succeeds_on_second_attempt_after_one_delay()
    {
        var primary = new Choice("Win+Y");
        var fallback = new Choice("Ctrl+Alt+Y");
        var delays = new List<TimeSpan>();
        var attempts = 0;

        var (ok, chosen) = HotkeyRegistration.RegisterWithRetry(
            new[] { primary, fallback },
            c =>
            {
                if (c == primary)
                {
                    attempts++;
                    return attempts == 2;
                }
                return true;
            },
            delay: d => delays.Add(d));

        Assert.True(ok);
        Assert.Same(primary, chosen);
        Assert.Equal(2, attempts);
        Assert.Single(delays);
        Assert.Equal(HotkeyRegistration.DefaultRetryDelay, delays[0]);
    }

    [Fact]
    public void Retries_primary_and_succeeds_on_third_attempt_after_two_delays()
    {
        var primary = new Choice("Win+Y");
        var fallback = new Choice("Ctrl+Alt+Y");
        var delays = new List<TimeSpan>();
        var attempts = 0;

        var (ok, chosen) = HotkeyRegistration.RegisterWithRetry(
            new[] { primary, fallback },
            c =>
            {
                if (c == primary)
                {
                    attempts++;
                    return attempts == 3;
                }
                return true;
            },
            delay: d => delays.Add(d));

        Assert.True(ok);
        Assert.Same(primary, chosen);
        Assert.Equal(3, attempts);
        Assert.Equal(2, delays.Count);
        Assert.All(delays, d => Assert.Equal(HotkeyRegistration.DefaultRetryDelay, d));
    }

    [Fact]
    public void Falls_back_to_secondary_choice_when_primary_exhausts_retries()
    {
        var primary = new Choice("Win+Y");
        var fallback = new Choice("Ctrl+Alt+Y");
        var delays = new List<TimeSpan>();
        var registerCalls = new List<string>();

        var (ok, chosen) = HotkeyRegistration.RegisterWithRetry(
            new[] { primary, fallback },
            c =>
            {
                registerCalls.Add(c.Label);
                return c == fallback;
            },
            delay: d => delays.Add(d));

        Assert.True(ok);
        Assert.Same(fallback, chosen);
        Assert.Equal(2, delays.Count);
        Assert.Equal(new[] { "Win+Y", "Win+Y", "Win+Y", "Ctrl+Alt+Y" }, registerCalls);
    }

    [Fact]
    public void Falls_back_through_multiple_secondary_choices_in_order()
    {
        var primary = new Choice("Win+Y");
        var fallback1 = new Choice("Ctrl+Alt+Y");
        var fallback2 = new Choice("Ctrl+Shift+Y");
        var registerCalls = new List<string>();

        var (ok, chosen) = HotkeyRegistration.RegisterWithRetry(
            new[] { primary, fallback1, fallback2 },
            c =>
            {
                registerCalls.Add(c.Label);
                return c == fallback2;
            },
            delay: _ => { });

        Assert.True(ok);
        Assert.Same(fallback2, chosen);
        Assert.Equal(new[] { "Win+Y", "Win+Y", "Win+Y", "Ctrl+Alt+Y", "Ctrl+Shift+Y" }, registerCalls);
    }

    [Fact]
    public void Returns_false_when_all_choices_fail()
    {
        var primary = new Choice("Win+Y");
        var fallback = new Choice("Ctrl+Alt+Y");
        var delays = 0;

        var (ok, chosen) = HotkeyRegistration.RegisterWithRetry(
            new[] { primary, fallback },
            _ => false,
            delay: _ => delays++);

        Assert.False(ok);
        Assert.Null(chosen);
        Assert.Equal(2, delays);
    }

    [Fact]
    public void Respects_custom_attempts_and_delay()
    {
        var primary = new Choice("Win+Y");
        var delays = new List<TimeSpan>();
        var customDelay = TimeSpan.FromMilliseconds(150);

        var (ok, _) = HotkeyRegistration.RegisterWithRetry(
            new[] { primary },
            _ => false,
            primaryAttempts: 5,
            retryDelay: customDelay,
            delay: d => delays.Add(d));

        Assert.False(ok);
        Assert.Equal(4, delays.Count);
        Assert.All(delays, d => Assert.Equal(customDelay, d));
    }
}
