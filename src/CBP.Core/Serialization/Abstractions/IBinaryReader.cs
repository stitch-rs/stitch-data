namespace CBP.Serialization.Abstractions;

/// <summary>
/// Provides methods for reading primitive types and structures from compact binary format
/// </summary>
public interface IBinaryReader
{
    /// <summary>
    /// Reads a single byte
    /// </summary>
    byte ReadByte();
    
    /// <summary>
    /// Reads a boolean value
    /// </summary>
    bool ReadBool();
    
    /// <summary>
    /// Reads an integer using variable-length encoding with ZigZag decoding
    /// </summary>
    int ReadCompactInt();
    
    /// <summary>
    /// Reads a string using UTF-8 encoding with length prefix
    /// </summary>
    string ReadCompactString();
    
    /// <summary>
    /// Reads a string with fixed buffer size
    /// </summary>
    /// <param name="fixedLength">Fixed buffer length</param>
    string ReadFixedString(int fixedLength);
    
    /// <summary>
    /// Reads a single-precision floating point value
    /// </summary>
    float ReadFloat();
    
    /// <summary>
    /// Reads a double-precision floating point value
    /// </summary>
    double ReadDouble();
    
    /// <summary>
    /// Reads raw byte data
    /// </summary>
    /// <param name="length">Number of bytes to read</param>
    ReadOnlySpan<byte> ReadRaw(int length);
}
