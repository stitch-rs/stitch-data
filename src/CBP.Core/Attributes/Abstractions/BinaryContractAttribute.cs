using System;
using CBP.Enums;

namespace CBP.Attributes.Abstractions;

/// <summary>
/// Specifies that a type is a binary serialization contract
/// </summary>
[AttributeUsage(AttributeTargets.Struct | AttributeTargets.Class)]
public sealed class BinaryContractAttribute : Attribute
{
    /// <summary>
    /// Contract version for schema evolution
    /// </summary>
    public int Version { get; set; } = 1;
    
    /// <summary>
    /// Serialization mode for this contract
    /// </summary>
    public SerializationMode Mode { get; set; } = SerializationMode.Compact;
    
    /// <summary>
    /// Optional name for the contract (uses type name if not specified)
    /// </summary>
    public string? Name { get; set; }
}