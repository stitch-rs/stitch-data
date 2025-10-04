using System;

namespace CBP.Serialization.Abstractions;

/// <summary>
/// Abstract base class for binary writers with common functionality
/// </summary>
public abstract class BinaryWriterBase : IBinaryWriter
{
    /// <summary>
    /// Gets the serialization options for this writer
    /// </summary>
    protected CBP.Configuration.Abstractions.CbpSerializationOptions Options { get; }
    
    /// <summary>
    /// Initializes a new instance of the BinaryWriterBase class
    /// </summary>
    /// <param name="options">Serialization options</param>
    protected BinaryWriterBase(CBP.Configuration.Abstractions.CbpSerializationOptions? options = null)
    {
        Options = options ?? CBP.Configuration.Abstractions.CbpSerializationOptions.Default;
    }
    
    /// <inheritdoc/>
    public abstract void WriteByte(byte value);
    
    /// <inheritdoc/>
    public abstract void WriteBool(bool value);
    
    /// <inheritdoc/>
    public abstract void WriteCompactInt(int value);
    
    /// <inheritdoc/>
    public abstract void WriteCompactString(string value);
    
    /// <inheritdoc/>
    public abstract void WriteFixedString(string value, int fixedLength);
    
    /// <inheritdoc/>
    public abstract void WriteFloat(float value);
    
    /// <inheritdoc/>
    public abstract void WriteDouble(double value);
    
    /// <inheritdoc/>
    public abstract void WriteRaw(ReadOnlySpan<byte> data);
    
    /// <summary>
    /// Writes an unsigned variable-length integer
    /// </summary>
    protected abstract void WriteUnsignedVarInt(uint value);
}
