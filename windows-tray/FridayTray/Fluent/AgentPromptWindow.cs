using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using FridayTray.Core;
using Wpf.Ui.Controls;
using Brush = System.Windows.Media.Brush;
using Button = System.Windows.Controls.Button;
using FontFamily = System.Windows.Media.FontFamily;
using HorizontalAlignment = System.Windows.HorizontalAlignment;
using Orientation = System.Windows.Controls.Orientation;
using TextBlock = System.Windows.Controls.TextBlock;
using TextBox = System.Windows.Controls.TextBox;
using VerticalAlignment = System.Windows.VerticalAlignment;

namespace FridayTray.Fluent;

/// <summary>
/// A prompt asking the person to allow or deny a coding agent's risky step,
/// or to reply to a question from the agent.
/// </summary>
internal sealed class AgentPromptWindow : CardWindow
{
    public AgentPromptWindow(string agentsDir, AgentRun run)
        : base("FRIDAY · " + run.Agent, run.IsQuestion ? "Agent question" : "Agent approval", 480)
    {
        var details = new StackPanel();
        var header = new TextBlock
        {
            Text = $"{run.Agent} · {run.Folder}",
            FontSize = 12.5,
            Foreground = R("Subtext"),
            TextTrimming = TextTrimming.CharacterEllipsis,
            Margin = new Thickness(0, 0, 0, 8),
        };
        AutomationProperties.SetName(header, "Agent and folder");
        details.Children.Add(header);

        var stepBlock = new TextBlock
        {
            Text = run.Pending?.Step ?? "",
            FontSize = run.IsQuestion ? 14 : 13.5,
            FontWeight = FontWeights.SemiBold,
            Foreground = R("Text"),
            TextWrapping = TextWrapping.Wrap,
            Margin = new Thickness(0, 0, 0, 4),
        };
        if (!run.IsQuestion)
            stepBlock.FontFamily = (FontFamily)FindResource("MonoFont");
        AutomationProperties.SetName(stepBlock, run.IsQuestion ? "Question" : "Step");
        details.Children.Add(stepBlock);

        if (!string.IsNullOrEmpty(run.Pending?.Why))
        {
            var whyBlock = new TextBlock
            {
                Text = run.Pending.Why,
                FontSize = 12.5,
                Foreground = R("Subtext"),
                TextWrapping = TextWrapping.Wrap,
            };
            AutomationProperties.SetName(whyBlock, "Reason");
            details.Children.Add(whyBlock);
        }

        Body.Children.Add(Well(details));

        if (run.IsQuestion)
        {
            var replyBox = new TextBox
            {
                FontSize = 13,
                Foreground = R("Text"),
                CaretBrush = R("Ember"),
                Background = R("Well"),
                BorderBrush = R("Hairline"),
                BorderThickness = new Thickness(1),
                Padding = new Thickness(10, 8, 10, 8),
                Margin = new Thickness(0, 4, 0, 10),
                TextWrapping = TextWrapping.Wrap,
                AcceptsReturn = false,
            };
            AutomationProperties.SetName(replyBox, "Your reply");
            Body.Children.Add(replyBox);
            Loaded += (_, _) => replyBox.Focus();

            var deny = Pill("Deny");
            deny.Click += (_, _) =>
            {
                AgentRuns.Answer(agentsDir, run.Id, "deny");
                Close();
            };

            var reply = Pill("Reply", SymbolRegular.Send24, primary: true);
            reply.IsDefault = true;
            reply.Click += (_, _) =>
            {
                var text = replyBox.Text.Trim();
                if (text.Length > 0)
                {
                    AgentRuns.Answer(agentsDir, run.Id, $"reply:{text}");
                    Close();
                }
            };

            Buttons.Children.Add(deny);
            Buttons.Children.Add(reply);
        }
        else
        {
            var deny = Pill("Deny");
            deny.Click += (_, _) =>
            {
                AgentRuns.Answer(agentsDir, run.Id, "deny");
                Close();
            };

            var allow = Pill("Allow", SymbolRegular.Checkmark24, primary: true);
            allow.IsDefault = true;
            allow.Click += (_, _) =>
            {
                AgentRuns.Answer(agentsDir, run.Id, "allow");
                Close();
            };

            Buttons.Children.Add(deny);
            Buttons.Children.Add(allow);
        }
    }
}
