using System;

namespace MutexFlow.Desktop.Models;

public enum InferenceMode
{
    Local,
    Standard,
    Byok
}

public enum WorkflowType
{
    GeneralReasoning,
    DataExtraction,
    ProfessionalDrafting
}

public enum TaskComplexity
{
    Low,
    Medium,
    High,
    Reasoning
}

public enum LogType
{
    System,
    Success,
    Error
}

public class LogEntry
{
    public string Text { get; set; } = string.Empty;
    public LogType Type { get; set; } = LogType.System;
    public DateTime Timestamp { get; set; } = DateTime.Now;

    public LogEntry(string text, LogType type = LogType.System)
    {
        Text = text;
        Type = type;
    }
}
