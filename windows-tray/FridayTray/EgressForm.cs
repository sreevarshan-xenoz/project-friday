using FridayTray.Core;

namespace FridayTray;

/// <summary>
/// Simple window listing outbound AI requests ("What left this PC…").
/// Lists entries newest first; never shows prompt or content.
/// </summary>
internal sealed class EgressForm : Form
{
    public EgressForm(Paths paths)
    {
        Theme.DarkTitleBar(this);
        Text = "FRIDAY · What left this PC";
        FormBorderStyle = FormBorderStyle.Sizable;
        StartPosition = FormStartPosition.CenterScreen;
        BackColor = Theme.Bg;
        ForeColor = Theme.Text;
        Font = Theme.Body;
        ClientSize = new Size(720, 480);
        MinimumSize = new Size(500, 300);
        Padding = new Padding(16);

        var entries = EgressLog.ReadNewestFirst(paths.EgressFile);
        var summaryText = EgressLog.FormatSummary(entries, DateTime.UtcNow);

        var layout = new TableLayoutPanel
        {
            Dock = DockStyle.Fill,
            ColumnCount = 1,
            RowCount = 3,
            BackColor = Theme.Bg,
        };
        layout.RowStyles.Add(new RowStyle(SizeType.AutoSize));
        layout.RowStyles.Add(new RowStyle(SizeType.Percent, 100f));
        layout.RowStyles.Add(new RowStyle(SizeType.AutoSize));

        var summaryLabel = new Label
        {
            Text = summaryText,
            AutoSize = true,
            ForeColor = Theme.Accent,
            Font = Theme.Title,
            Margin = new Padding(0, 0, 0, 12),
        };
        layout.Controls.Add(summaryLabel, 0, 0);

        if (entries.Count == 0)
        {
            var emptyLabel = new Label
            {
                Text = "Nothing has been sent yet.",
                Dock = DockStyle.Fill,
                TextAlign = ContentAlignment.MiddleCenter,
                ForeColor = Theme.Subtext,
                Font = Theme.Body,
            };
            layout.Controls.Add(emptyLabel, 0, 1);
        }
        else
        {
            var list = new ListView
            {
                Dock = DockStyle.Fill,
                View = View.Details,
                FullRowSelect = true,
                BackColor = Theme.Surface,
                ForeColor = Theme.Text,
                BorderStyle = BorderStyle.None,
                Font = Theme.Small,
            };

            list.Columns.Add("Time", 140);
            list.Columns.Add("Provider", 110);
            list.Columns.Add("Model", 130);
            list.Columns.Add("What", 200);
            list.Columns.Add("Masked", 60);
            list.Columns.Add("Outcome", 60);

            foreach (var e in entries)
            {
                var item = new ListViewItem(EgressLog.FormatLocalTime(e.Ts));
                item.SubItems.Add(EgressLog.FormatProvider(e.Provider));
                item.SubItems.Add(e.Model);
                item.SubItems.Add(EgressLog.FormatWhat(e.TextChars, e.ImageBytes));
                item.SubItems.Add((e.Masked ?? 0).ToString());
                item.SubItems.Add(e.Outcome);
                list.Items.Add(item);
            }
            layout.Controls.Add(list, 0, 1);
        }

        var btnPanel = new FlowLayoutPanel
        {
            Dock = DockStyle.Fill,
            FlowDirection = FlowDirection.RightToLeft,
            Height = 40,
            BackColor = Theme.Bg,
            Margin = new Padding(0, 8, 0, 0),
        };
        var closeBtn = new Button
        {
            Text = "Close",
            BackColor = Theme.Surface,
            ForeColor = Theme.Text,
            FlatStyle = FlatStyle.Flat,
            Height = 32,
            Width = 80,
        };
        closeBtn.FlatAppearance.BorderColor = Theme.Outline;
        closeBtn.Click += (_, _) => Close();
        btnPanel.Controls.Add(closeBtn);
        layout.Controls.Add(btnPanel, 0, 2);

        Controls.Add(layout);
    }
}
