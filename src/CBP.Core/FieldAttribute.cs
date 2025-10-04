using System;

namespace CBP;

/// <summary>
/// Specifies field metadata for binary serialization
/// </summary>
[AttributeUsage(AttributeTargets.Field | AttributeTargets.Property)]
public sealed class FieldAttribute : Attribute
{
    /// <summary>
    /// Field tag for versioning and schema evolution
    /// </summary>
    public int Tag { get; }
    
    /// <summary>
    /// Version when this field was introduced
    /// </summary>
    public int SinceVersion { get; set; } = 1;
    
    /// <summary>
    /// Fixed length for strings and arrays (-1 for variable length)
    /// </summary>
    public int FixedLength { get; set; } = -1;
    
    /// <summary>
    /// Field-specific optimization hints
    /// </summary>
    public FieldAccess Access { get; set; } = FieldAccess.Default;
    
    /// <summary>
    /// Initializes a new instance of the FieldAttribute class
    /// </summary>
    /// <param name="tag">Field tag for versioning</param>
    public FieldAttribute(int tag)
    {
        Tag = tag;
    }
}
