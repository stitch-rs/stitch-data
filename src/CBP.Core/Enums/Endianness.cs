namespace CBP.Enums;

/// <summary>
/// Byte order for binary data
/// </summary>
public enum Endianness
{
    /// <summary>
    /// Little-endian byte order (x86, x64)
    /// </summary>
    Little,
    
    /// <summary>
    /// Big-endian byte order (network order)
    /// </summary>
    Big,
    
    /// <summary>
    /// Use native system endianness
    /// </summary>
    Native
}
