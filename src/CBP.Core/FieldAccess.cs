namespace CBP;

/// <summary>
/// Field-specific serialization optimization hints
/// </summary>
public enum FieldAccess
{
    /// <summary>
    /// Use default serialization strategy
    /// </summary>
    Default,
    
    /// <summary>
    /// Optimize for zero-copy access to large data
    /// </summary>
    ZeroCopy,
    
    /// <summary>
    /// Apply performance optimizations
    /// </summary>
    Optimized
}
