using System;
using System.IO;
using System.Security.Cryptography;
using System.Text;

namespace MutexFlow.Desktop.Services;

public interface ISecureStorageService
{
    void SaveSecret(string key, string secret);
    string? GetSecret(string key);
    void RemoveSecret(string key);
}

public class SecureStorageService : ISecureStorageService
{
    private readonly string _storageDir;

    public SecureStorageService()
    {
        _storageDir = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
            "MutexFlow",
            "secure"
        );
        Directory.CreateDirectory(_storageDir);
        RestrictUnixDirectoryPermissions(_storageDir);
    }

    public void SaveSecret(string key, string secret)
    {
        string? temporaryPath = null;
        try
        {
            var filePath = GetSecretPath(key);
            var plaintextBytes = Encoding.UTF8.GetBytes(secret);
            var bytesToWrite = OperatingSystem.IsWindows()
                ? ProtectedData.Protect(plaintextBytes, null, DataProtectionScope.CurrentUser)
                : plaintextBytes;

            temporaryPath = filePath + "." + Guid.NewGuid().ToString("N") + ".tmp";
            using (var stream = new FileStream(temporaryPath, FileMode.CreateNew, FileAccess.Write, FileShare.None))
            {
                RestrictUnixFilePermissions(temporaryPath);
                stream.Write(bytesToWrite, 0, bytesToWrite.Length);
                stream.Flush(flushToDisk: true);
            }
            File.Move(temporaryPath, filePath, overwrite: true);
            temporaryPath = null;
            RestrictUnixFilePermissions(filePath);
        }
        catch (Exception ex)
        {
            System.Diagnostics.Debug.WriteLine($"[SecureStorage] Save error: {ex.Message}");
        }
        finally
        {
            if (temporaryPath != null)
            {
                try { File.Delete(temporaryPath); } catch { }
            }
        }
    }

    public string? GetSecret(string key)
    {
        try
        {
            var filePath = GetSecretPath(key);
            if (!File.Exists(filePath))
                return null;

            RestrictUnixFilePermissions(filePath);
            var bytes = File.ReadAllBytes(filePath);
            return Encoding.UTF8.GetString(
                OperatingSystem.IsWindows()
                    ? ProtectedData.Unprotect(bytes, null, DataProtectionScope.CurrentUser)
                    : bytes
            );
        }
        catch
        {
            return null;
        }
    }

    public void RemoveSecret(string key)
    {
        try
        {
            var filePath = GetSecretPath(key);
            if (File.Exists(filePath))
                File.Delete(filePath);
        }
        catch { }
    }

    private string GetSecretPath(string key)
    {
        if (string.IsNullOrWhiteSpace(key) ||
            !key.All(character => char.IsAsciiLetterOrDigit(character) || character is '_' or '-'))
            throw new ArgumentException("Secret keys may contain only ASCII letters, digits, underscores, and hyphens.", nameof(key));
        return Path.Combine(_storageDir, key + ".dat");
    }

    private static void RestrictUnixDirectoryPermissions(string path)
    {
        if (!OperatingSystem.IsWindows())
            File.SetUnixFileMode(path, UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute);
    }

    private static void RestrictUnixFilePermissions(string path)
    {
        if (!OperatingSystem.IsWindows())
            File.SetUnixFileMode(path, UnixFileMode.UserRead | UnixFileMode.UserWrite);
    }
}
