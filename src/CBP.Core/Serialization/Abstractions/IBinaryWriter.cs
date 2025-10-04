namespace CBP.Serialization.Abstractions;

/// <summary>
/// Provides methods for writing primitive types and structures in compact binary format
/// </summary>
public interface IBinaryWriter
{
    /// <summary>
    /// Writes a single byte
    /// </summary>
    void WriteByte(byte value);
    
    /// <summary>
    /// Writes a boolean value (1 byte)
    /// </summary>
    void WriteBool(bool value);
    
    /// <summary>
    /// Writes an integer using variable-length encoding with ZigZag for negative values
    /// </summary>
    void WriteCompactInt(int value);
    
    /// <summary>
    /// Writes a string using UTF-8 encoding with length prefix
    /// </summary>
    void WriteCompactString(string value);
    
    /// <summary>
    /// Writes a string with fixed buffer size
    /// </summary>
    /// <param name="value">String value to write</param>
    /// <param name="fixedLength">Fixed buffer length</param>
    void WriteFixedString(string value, int fixedLength);
    
    /// <summary>
    /// Writes a single-precision floating point value
    /// </summary>
    void WriteFloat(float value);
    
    /// <summary>
    /// Writes a double-precision floating point value
    /// </summary>
    void WriteDouble(double value);
    
    /// <summary>
    /// Writes raw byte data
    /// </summary>
    void WriteRaw(ReadOnlySpan<byte> data);
}
