// Plays a game on the Emuera.NET fork's script engine with a list of
// inputs and a seed, and prints what it printed (Emuera's log text), for
// comparison with `emuera-tty --headless` (tools/net_replay.sh).
//
//   NetReplay GAME_DIR [--seed N] [--inputs FILE]
//
// Inputs are answered in order, one line each, the way emuera-tty's headless
// mode reads stdin: waits (WAIT, WAITANYKEY, TWAIT) pass by themselves and
// take no line. The run ends when the inputs run out, at QUIT, or at a
// script error (exit status 1). Saves the game writes are kept in its
// folder's sav/, so run it on a copy of the game.
using System.Reflection;
using MinorShift.Emuera.Web.Runtime;

static int Fail(string message)
{
    Console.Error.WriteLine($"net_replay: {message}");
    return 2;
}

string? gameDir = null;
string? inputsFile = null;
long seed = 0;
for (int i = 0; i < args.Length; i++)
{
    switch (args[i])
    {
        case "--seed" when i + 1 < args.Length && long.TryParse(args[i + 1], out var s):
            seed = s;
            i++;
            break;
        case "--inputs" when i + 1 < args.Length:
            inputsFile = args[++i];
            break;
        default:
            if (gameDir is not null || args[i].StartsWith("--"))
                return Fail($"unexpected argument '{args[i]}'\nusage: NetReplay GAME_DIR [--seed N] [--inputs FILE]");
            gameDir = args[i];
            break;
    }
}
if (gameDir is null || !Directory.Exists(gameDir))
    return Fail("usage: NetReplay GAME_DIR [--seed N] [--inputs FILE]");
gameDir = Path.GetFullPath(gameDir);

var inputs = new Queue<string>();
if (inputsFile is not null)
{
    try
    {
        foreach (var line in File.ReadAllLines(inputsFile))
            inputs.Enqueue(line.TrimEnd('\r'));
    }
    catch (Exception e) when (e is IOException or UnauthorizedAccessException)
    {
        return Fail($"cannot read {inputsFile}: {e.Message}");
    }
}

// the saves already in the copy (global.sav, saveNN.sav), handed to the
// runtime the way its browser host restores them
var saves = new Dictionary<string, byte[]>();
string savDir = Path.Combine(gameDir, "sav");
if (Directory.Exists(savDir))
{
    foreach (var path in Directory.GetFiles(savDir, "*.sav"))
    {
        string name = Path.GetFileName(path);
        if (name.Equals("global.sav", StringComparison.OrdinalIgnoreCase) || name.StartsWith("save", StringComparison.OrdinalIgnoreCase))
            saves[name] = File.ReadAllBytes(path);
    }
}

BrowserRuntimeSession session;
try
{
    session = await BrowserRuntimeSession.StartSavePersistentBootstrapAsync(gameDir, saves.Count > 0 ? saves : null);
    SeedRandom(seed);
    session.StartTitle();
}
catch (Exception e)
{
    Console.Out.Write(Normalize(PartialOutput()));
    return Fail($"start failed: {e.GetType().Name}: {e.Message}");
}

// Answer the runtime until the inputs run out or the game stops. Each step
// either passes a wait, acknowledges a save, or submits one input; the step
// limit keeps a game that waits forever from hanging the run.
const int MaxSteps = 1_000_000;
int exit = 0;
for (int step = 0; step < MaxSteps; step++)
{
    if (session.IsFailed)
    {
        var d = session.FailureDiagnostic;
        Console.Out.Write(Normalize(session.Output));
        Console.Error.WriteLine($"net_replay: script error at {d.ErbPosition} in @{d.CurrentFunction}: {d.ExceptionMessage}");
        return 1;
    }
    if (session.IsComplete)
        break;
    if (session.Status == BrowserRuntimeStatus.Persisting && session.PendingPersistence is { } put)
    {
        session.AcknowledgePersistence(put.OperationId);
        continue;
    }
    if (session.Status == BrowserRuntimeStatus.AwaitingHost && session.PendingAwait is { } wait)
    {
        if (!session.CompleteAwait(wait.RequestId, wait.SessionGeneration))
            return Fail("the runtime refused to go on after a wait");
        continue;
    }
    if (session.PendingInput is not { } prompt)
        return Fail($"stuck in state {session.Status}");
    // NET_REPLAY_TRACE=1: each prompt on stderr, to line inputs up
    if (Environment.GetEnvironmentVariable("NET_REPLAY_TRACE") is "1")
        Console.Error.WriteLine($"[prompt {prompt.Kind} one={prompt.OneInput} time={prompt.TimeLimit}; next input: {(inputs.Count > 0 ? inputs.Peek() : "(none)")}]");
    // a timed wait (TWAIT) ends by running out, not by an answer
    if (prompt.Kind is BrowserInputKind.TimedVoid)
    {
        if (!session.SubmitTimeout(prompt.RequestId))
            return Fail("the runtime refused to end a timed wait");
        continue;
    }
    if (prompt.Kind is BrowserInputKind.Enter or BrowserInputKind.AnyKey)
    {
        session.Submit(new InputEnvelope(prompt.RequestId, ""));
        continue;
    }
    if (inputs.Count == 0)
        break;
    string answer = inputs.Dequeue();
    bool taken = prompt.Kind == BrowserInputKind.MouseKey
        ? session.SubmitPrimitive(MouseKeyAnswer(prompt.RequestId, answer))
        : session.Submit(new InputEnvelope(prompt.RequestId, answer));
    // (a refused answer, a word for INPUT, is dropped as Emuera drops it)
    _ = taken;
}
Console.Out.Write(Normalize(session.Output));
return exit;

// RANDOMIZE seed, before the first line runs: the runtime seeds from the
// clock, and has no public way in, so its VariableEvaluator is reached the
// way GlobalStatic holds it.
static void SeedRandom(long seed)
{
    var asm = typeof(BrowserRuntimeSession).Assembly;
    var globalStatic = asm.GetTypes().FirstOrDefault(t => t.Name == "GlobalStatic")
        ?? throw new InvalidOperationException("GlobalStatic not found in the runtime");
    var evaluator = globalStatic.GetField("VEvaluator", BindingFlags.Static | BindingFlags.Public | BindingFlags.NonPublic)?.GetValue(null)
        ?? throw new InvalidOperationException("the runtime has no VariableEvaluator yet");
    var randomize = evaluator.GetType().GetMethod("Randomize", BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic, [typeof(long)])
        ?? throw new InvalidOperationException("VariableEvaluator.Randomize not found");
    randomize.Invoke(evaluator, [seed]);
}

static string PartialOutput() => "";

// INPUTMOUSEKEY, answered as emuera-tty's headless mode does (Frontend::
// input_raw): a number clicks the button with that value (left button), any
// other text presses its first character's key, an empty line Enter.
static PrimitiveInputEnvelope MouseKeyAnswer(long requestId, string answer)
{
    const int LeftButton = 0x100000;
    string text = answer.Trim();
    if (long.TryParse(text, out _))
        return new PrimitiveInputEnvelope(requestId, BrowserPrimitiveInputKind.Click, text, Code: LeftButton);
    var (code, data) = text.Length == 0 ? (13, 13) : KeyCode(text[0]);
    return new PrimitiveInputEnvelope(requestId, BrowserPrimitiveInputKind.Key, Code: code, Result2: data);
}

// the key code and key data of a typed character (emuera-tty's char_keycode)
static (int, int) KeyCode(char c) => c switch
{
    >= 'a' and <= 'z' => (char.ToUpperInvariant(c), char.ToUpperInvariant(c)),
    >= 'A' and <= 'Z' => (c, c | 0x10000),
    '\r' or '\n' => (13, 13),
    _ => (c, c),
};

// Line ends as \n, nothing else changed (comparison is the script's job).
static string Normalize(string text) => text.Replace("\r\n", "\n");
