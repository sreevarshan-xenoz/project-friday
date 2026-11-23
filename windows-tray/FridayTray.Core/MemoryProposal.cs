using System.Text.RegularExpressions;

namespace FridayTray.Core;

/// <summary>
/// What a question says about you, worth remembering ("I use fish, how do
/// I …" → "you use fish"), as `friday_memory_proposal` on Linux. The answer
/// card offers it; nothing is kept unless you say yes.
/// </summary>
public static class MemoryProposal
{
    private static readonly Regex Uses = new(@"(?:^|[^a-z'])(i use|i'm using|i am using|i prefer|i work with|i'm on|i am on|i run) ([a-z0-9][^,.?!;:]*)");
    private static readonly Regex Mine = new(@"(?:^|[^a-z])my (shell|editor|distro|os|terminal|browser|language|ide|laptop|keyboard layout|name) is ([a-z0-9][^,.?!;:]*)");
    private static readonly HashSet<string> Stop =
        ["and", "but", "so", "how", "what", "why", "can", "could", "to", "for", "with", "on", "in", "at", "when", "where", "which", "is"];
    private static readonly HashSet<string> Nothing = ["it", "this", "that", "these", "those", "them", "a", "an", "the"];

    /// <summary>"you use fish", "your editor is helix", or null.</summary>
    public static string? From(string question)
    {
        var q = question.ToLowerInvariant();
        string verb, what;
        if (Uses.Match(q) is { Success: true } u)
        {
            verb = u.Groups[1].Value switch { "i'm using" or "i am using" => "i use", "i am on" => "i'm on", var v => v };
            what = u.Groups[2].Value;
        }
        else if (Mine.Match(q) is { Success: true } m)
        {
            verb = $"my {m.Groups[1].Value} is";
            what = m.Groups[2].Value;
        }
        else
            return null;
        // At most three words, up to the first one that starts the question.
        var words = what.Split(' ', StringSplitOptions.RemoveEmptyEntries).TakeWhile(w => !Stop.Contains(w)).Take(3).ToList();
        if (words.Count == 0 || Nothing.Contains(words[0]))
            return null;
        var thing = string.Join(' ', words);
        if (Asking.LooksLikeSecret(thing))
            return null;
        return verb switch
        {
            "i use" => $"you use {thing}",
            "i prefer" => $"you prefer {thing}",
            "i work with" => $"you work with {thing}",
            "i'm on" => $"you're on {thing}",
            "i run" => $"you run {thing}",
            _ => $"your {verb[3..]} {thing}",
        };
    }

    /// <summary>The proposal in the person's own words, for memory ("I use fish").</summary>
    public static string InOwnWords(string proposal) =>
        proposal.StartsWith("you're ") ? "I'm " + proposal[7..]
        : proposal.StartsWith("your ") ? "my " + proposal[5..]
        : proposal.StartsWith("you ") ? "I " + proposal[4..]
        : proposal;
}
