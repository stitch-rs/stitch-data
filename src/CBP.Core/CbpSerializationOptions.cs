namespace CBP;

/// <summary>
/// Configuration options for CBP serialization
/// </summary>
public sealed class CbpSerializationOptions
{
    /// <summary>
    /// Serialization strategy mode
    /// </summary>
    public SerializationMode Mode { get; set; } = SerializationMode.Compact;
    
    /// <summary>
    /// Byte order for binary data
    /// </summary>
    public Endianness Endianness { get; set; } = Endianness.Little;
    
    /// <summary>
    /// Enable compression for large data
    /// </summary>
    public bool EnableCompression { get; set; } = false;
    
    /// <summary>
    /// Minimum data size in bytes to apply compression
    /// </summary>
    public int CompressionThreshold { get; set; } = 1024;
    
    /// <summary>
    /// Enable checksums for data integrity verification
    /// </summary>
    public bool EnableChecksums { get; set; } = false;
    
    /// <summary>
    /// Creates a new instance with default options
    /// </summary>
    public static CbpSerializationOptions Default => new();
}
