namespace CBP;

/// <summary>
/// Defines the serialization strategy and optimization goals
/// </summary>
public enum SerializationMode
{
    /// <summary>
    /// Maximum data compactness (default)
    /// </summary>
    Compact,
    
    /// <summary>
    /// Zero-copy access for large data
    /// </summary>
    ZeroCopy,
    
    /// <summary>
    /// Protocol compatibility and schema evolution
    /// </summary>
    Compatible,
    
    /// <summary>
    /// Maximum serialization/deserialization speed
    /// </summary>
    HighPerf
}
