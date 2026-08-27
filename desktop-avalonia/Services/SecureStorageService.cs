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
    }

    public void SaveSecret(string key, string secret)
    {
        try
        {
            var filePath = Path.Combine(_storageDir, key + ".dat");
            byte[] plaintextBytes = Encoding.UTF8.GetBytes(secret);

            if (OperatingSystem.IsWindows())
            {
                byte[] encrypted = ProtectedData.Protect(plaintextBytes, null, DataProtectionScope.CurrentUser);
                File.WriteAllBytes(filePath, encrypted);
            }
            else
            {
                File.WriteAllBytes(filePath, plaintextBytes);
            }
        }
        catch (Exception ex)
        {
            System.Diagnostics.Debug.WriteLine($"[SecureStorage] Save error: {ex.Message}");
        }
    }

    public string? GetSecret(string key)
    {
        try
        {
            var filePath = Path.Combine(_storageDir, key + ".dat");
            if (!File.Exists(filePath))
                return null;

            byte[] encrypted = File.ReadAllBytes(filePath);

            if (OperatingSystem.IsWindows())
            {
                byte[] decrypted = ProtectedData.Unprotect(encrypted, null, DataProtectionScope.CurrentUser);
                return Encoding.UTF8.GetString(decrypted);
            }
            else
            {
                return Encoding.UTF8.GetString(encrypted);
            }
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
            var filePath = Path.Combine(_storageDir, key + ".dat");
            if (File.Exists(filePath))
                File.Delete(filePath);
        }
        catch { }
    }
}
