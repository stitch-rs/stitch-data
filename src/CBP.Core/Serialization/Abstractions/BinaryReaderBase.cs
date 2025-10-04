using System;

namespace CBP.Serialization.Abstractions;

/// <summary>
/// Abstract base class for binary readers with common functionality
/// </summary>
public abstract class BinaryReaderBase : IBinaryReader
{
    /// <summary>
    /// Gets the serialization options for this reader
    /// </summary>
    protected CBP.Configuration.Abstractions.CbpSerializationOptions Options { get; }
    
    /// <summary>
    /// Initializes a new instance of the BinaryReaderBase class
    /// </summary>
    /// <param name="options">Serialization options</param>
    protected BinaryReaderBase(CBP.Configuration.Abstractions.CbpSerializationOptions? options = null)
    {
        Options = options ?? CBP.Configuration.Abstractions.CbpSerializationOptions.Default;
    }
    
    /// <inheritdoc/>
    public abstract byte ReadByte();
    
    /// <inheritdoc/>
    public abstract bool ReadBool();
    
    /// <inheritdoc/>
    public abstract int ReadCompactInt();
    
    /// <inheritdoc/>
    public abstract string ReadCompactString();
    
    /// <inheritdoc/>
    public abstract string ReadFixedString(int fixedLength);
    
    /// <inheritdoc/>
    public abstract float ReadFloat();
    
    /// <inheritdoc/>
    public abstract double ReadDouble();
    
    /// <inheritdoc/>
    public abstract ReadOnlySpan<byte> ReadRaw(int length);
    
    /// <summary>
    /// Reads an unsigned variable-length integer
    /// </summary>
    protected abstract uint ReadUnsignedVarInt();
}
