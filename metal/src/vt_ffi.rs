//! FFI a VideoToolbox / CoreVideo / CoreMedia (APIs C puras, sin ObjC).

#![allow(non_snake_case, non_camel_case_types, dead_code)]

use core_foundation::base::CFTypeID;
use std::ffi::{c_void, c_char};

pub type OSStatus = i32;
pub type CMTimeValue = i64;
pub type CMTimeScale = i32;
pub type CMItemCount = isize;
pub type FourCharCode = u32;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CMTime { pub value: CMTimeValue, pub timescale: CMTimeScale, pub flags: u32, pub epoch: i64 }

/// el sello que viaja CON la muestra. Sin esto `CMSampleBufferCreateReady`
/// se crea sin tiempos y el callback del decodificador recibe un pts
/// inventado: imposible reordenar lo que sale.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CMSampleTimingInfo {
    pub duration: CMTime,
    pub presentation_ts: CMTime,
    pub decode_ts: CMTime,
}

/// CMTime inválido (el que se pone donde no hay dato)
pub const CMTIME_INVALIDO: CMTime = CMTime { value: 0, timescale: 0, flags: 0, epoch: 0 };

pub const KVTVIDEOTYPE_HEVC: FourCharCode = u32::from_be_bytes(*b"hvc1");
pub const KCVPIXELFORMATTYPE_420YPCBCR10BIPLANARVIDEORANGE: u32 = u32::from_be_bytes(*b"x420");
pub const KVTVIDEOTYPE_PRORES4444: FourCharCode = u32::from_be_bytes(*b"ap4h");
pub const KVTVIDEOTYPE_PRORES422HQ: FourCharCode = u32::from_be_bytes(*b"apch");

pub type VTDecompressionSessionRef = *mut c_void;
pub type VTCompressionSessionRef = *mut c_void;
pub type CMVideoFormatDescriptionRef = *mut c_void;
pub type CMSampleBufferRef = *mut c_void;
pub type CMBlockBufferRef = *mut c_void;
pub type CVPixelBufferRef = *mut c_void;
pub type CVPixelBufferPoolRef = *mut c_void;
pub type CVMetalTextureCacheRef = *mut c_void;
pub type CVMetalTextureRef = *mut c_void;
pub type CFMutableDictionaryRef = *mut c_void;
pub type CFStringRef = *const c_void;
pub type CFNumberRef = *const c_void;
pub type CFBooleanRef = *const c_void;
pub type CFAllocatorRef = *const c_void;
pub type CFDictionaryRef = *const c_void;
pub type CFArrayRef = *const c_void;

pub type VTDecompressionOutputCallback = unsafe extern "C" fn(
    decompression_output_ref_con: *mut c_void,
    source_frame_ref_con: *mut c_void,
    status: OSStatus,
    info_flags: u32,
    image_buffer: CVPixelBufferRef,
    presentation_time_stamp: CMTime,
    presentation_duration: CMTime,
);

#[repr(C)]
pub struct VTDecompressionOutputCallbackRecord {
    pub decompression_output_callback: VTDecompressionOutputCallback,
    pub decompression_output_ref_con: *mut c_void,
}

pub type VTCompressionOutputCallback = unsafe extern "C" fn(
    output_callback_ref_con: *mut c_void,
    source_frame_ref_con: *mut c_void,
    status: OSStatus,
    info_flags: u32,
    sample_buffer: CMSampleBufferRef,
);

pub const KCVIMAGECHROMASUBSAMPLING_420: i32 = 1;

#[link(name = "VideoToolbox", kind = "framework")]
#[link(name = "CoreVideo", kind = "framework")]
#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    // decompression
    pub fn VTDecompressionSessionCreate(
        allocator: CFAllocatorRef,
        video_format_description: CMVideoFormatDescriptionRef,
        video_decoder_specification: CFDictionaryRef,
        destination_image_buffer_attributes: CFDictionaryRef,
        output_callback: *const VTDecompressionOutputCallbackRecord,
        decompression_session_out: *mut VTDecompressionSessionRef,
    ) -> OSStatus;
    pub fn VTDecompressionSessionDecodeFrame(
        session: VTDecompressionSessionRef,
        sample_buffer: CMSampleBufferRef,
        decode_flags: u32,
        source_frame_ref_con: *mut c_void,
        info_flags_out: *mut u32,
    ) -> OSStatus;
    pub fn VTDecompressionSessionWaitForAsynchronousFrames(session: VTDecompressionSessionRef) -> OSStatus;
    /// el taller también trae material en H.264 (móviles, pantallazos)
    pub fn CMVideoFormatDescriptionCreateFromH264ParameterSets(
        allocator: CFAllocatorRef, count: usize, pointers: *const *const u8,
        sizes: *const usize, nal_len: i32, out: *mut CMVideoFormatDescriptionRef) -> OSStatus;
    pub fn VTDecompressionSessionInvalidate(session: VTDecompressionSessionRef);

    // compression
    pub fn VTCompressionSessionCreate(
        allocator: CFAllocatorRef,
        width: i32, height: i32,
        codec_type: FourCharCode,
        encoder_specification: CFDictionaryRef,
        source_image_buffer_attributes: CFDictionaryRef,
        compressed_data_allocator: CFAllocatorRef,
        output_callback: VTCompressionOutputCallback,
        output_callback_ref_con: *mut c_void,
        compression_session_out: *mut VTCompressionSessionRef,
    ) -> OSStatus;
    pub fn VTCompressionSessionEncodeFrame(
        session: VTCompressionSessionRef,
        image_buffer: CVPixelBufferRef,
        presentation_time_stamp: CMTime,
        duration: CMTime,
        frame_properties: CFDictionaryRef,
        source_frame_ref_con: *mut c_void,
        info_flags_out: *mut u32,
    ) -> OSStatus;
    pub fn VTCompressionSessionCompleteFrames(session: VTCompressionSessionRef, complete_until_presentation_time_stamp: CMTime) -> OSStatus;
    pub fn VTCompressionSessionGetPixelBufferPool(session: VTCompressionSessionRef) -> CVPixelBufferPoolRef;
    pub fn VTCompressionSessionInvalidate(session: VTCompressionSessionRef);
    pub fn VTSessionSetProperty(session: *mut c_void, property_key: CFStringRef, property_value: *const c_void) -> OSStatus;

    // format description (HEVC desde NAL units)
    pub fn CMVideoFormatDescriptionCreateFromHEVCParameterSets(
        allocator: CFAllocatorRef,
        parameter_set_count: usize,
        parameter_set_pointers: *const *const u8,
        parameter_set_sizes: *const usize,
        nal_unit_header_length: i32,
        extensions: CFDictionaryRef,
        format_description_out: *mut CMVideoFormatDescriptionRef,
    ) -> OSStatus;

    // sample/block buffers
    pub fn CMBlockBufferCreateWithMemoryBlock(
        structure_allocator: CFAllocatorRef,
        memory_block: *mut c_void,
        block_length: usize,
        block_allocator: CFAllocatorRef,
        custom_block_source: *const c_void,
        offset_to_data: usize,
        data_length: usize,
        flags: u32,
        block_buffer_out: *mut CMBlockBufferRef,
    ) -> OSStatus;
    pub fn CMBlockBufferCreateWithBufferReference(
        structure_allocator: CFAllocatorRef,
        buffer_reference: CMBlockBufferRef,
        offset_to_data: usize,
        data_length: usize,
        flags: u32,
        block_buffer_out: *mut CMBlockBufferRef,
    ) -> OSStatus;
    pub fn CMBlockBufferGetDataLength(the_buffer: CMBlockBufferRef) -> usize;
    pub fn CMBlockBufferGetDataPointer(
        the_buffer: CMBlockBufferRef,
        offset: usize,
        length_pointer_at_offset: *mut usize,
        total_length_pointer: *mut usize,
        data_pointer: *mut *mut c_char,
    ) -> OSStatus;
    pub fn CMSampleBufferCreateReady(
        allocator: CFAllocatorRef,
        data_buffer: CMBlockBufferRef,
        format_description: CMVideoFormatDescriptionRef,
        num_samples: CMItemCount,
        num_sample_timing_entries: CMItemCount,
        sample_timing_array: *const c_void,
        num_sample_size_entries: CMItemCount,
        sample_size_array: *const usize,
        sample_buffer_out: *mut CMSampleBufferRef,
    ) -> OSStatus;
    pub fn CMSampleBufferGetDataBuffer(sbuf: CMSampleBufferRef) -> CMBlockBufferRef;
    pub fn CMSampleBufferGetPresentationTimeStamp(sbuf: CMSampleBufferRef) -> CMTime;
    pub fn CMSampleBufferGetFormatDescription(sbuf: CMSampleBufferRef) -> CMVideoFormatDescriptionRef;
    pub fn CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(
        desc: CMVideoFormatDescriptionRef,
        parameter_set_index: usize,
        parameter_set_pointer_out: *mut *const u8,
        parameter_set_size_out: *mut usize,
        parameter_set_count_out: *mut usize,
        nal_unit_header_length_out: *mut i32,
    ) -> OSStatus;

    // pixel buffers
    pub fn CVPixelBufferGetWidth(pixel_buffer: CVPixelBufferRef) -> usize;
    pub fn CVPixelBufferGetHeight(pixel_buffer: CVPixelBufferRef) -> usize;
    pub fn CVPixelBufferGetPixelFormatType(pixel_buffer: CVPixelBufferRef) -> u32;
    pub fn CVPixelBufferGetIOSurface(pixel_buffer: CVPixelBufferRef) -> *mut c_void;
    pub fn CVPixelBufferLockBaseAddress(pixel_buffer: CVPixelBufferRef, lock_flags: u64) -> OSStatus;
    pub fn CVPixelBufferUnlockBaseAddress(pixel_buffer: CVPixelBufferRef, lock_flags: u64) -> OSStatus;
    pub fn CVPixelBufferGetBaseAddress(pixel_buffer: CVPixelBufferRef) -> *mut c_void;
    pub fn CVPixelBufferGetBytesPerRow(pixel_buffer: CVPixelBufferRef) -> usize;
    pub fn CVPixelBufferGetBytesPerRowOfPlane(pixel_buffer: CVPixelBufferRef, plane_index: usize) -> usize;
    pub fn CVPixelBufferGetWidthOfPlane(pixel_buffer: CVPixelBufferRef, plane_index: usize) -> usize;
    pub fn CVPixelBufferGetHeightOfPlane(pixel_buffer: CVPixelBufferRef, plane_index: usize) -> usize;
    pub fn CVPixelBufferGetBaseAddressOfPlane(pixel_buffer: CVPixelBufferRef, plane_index: usize) -> *mut c_void;
    pub fn CVPixelBufferCreate(
        allocator: CFAllocatorRef,
        width: usize, height: usize,
        pixel_format_type: u32,
        pixel_buffer_attributes: CFDictionaryRef,
        pixel_buffer_out: *mut CVPixelBufferRef,
    ) -> OSStatus;
    pub fn CVPixelBufferPoolCreate(
        allocator: CFAllocatorRef,
        pool_attributes: CFDictionaryRef,
        pixel_buffer_attributes: CFDictionaryRef,
        pool_out: *mut CVPixelBufferPoolRef,
    ) -> OSStatus;
    pub fn CVPixelBufferPoolCreatePixelBuffer(
        allocator: CFAllocatorRef,
        pool: CVPixelBufferPoolRef,
        pixel_buffer_out: *mut CVPixelBufferRef,
    ) -> OSStatus;

    // metal texture cache
    pub fn CVMetalTextureCacheCreate(
        allocator: CFAllocatorRef,
        cache_attributes: CFDictionaryRef,
        metal_device: *mut c_void,
        texture_attributes: CFDictionaryRef,
        cache_out: *mut CVMetalTextureCacheRef,
    ) -> OSStatus;
    pub fn CVMetalTextureCacheCreateTextureFromImage(
        allocator: CFAllocatorRef,
        texture_cache: CVMetalTextureCacheRef,
        source_image: CVPixelBufferRef,
        texture_attributes: CFDictionaryRef,
        pixel_format: u64,
        width: usize, height: usize, plane_index: usize,
        texture_out: *mut CVMetalTextureRef,
    ) -> OSStatus;
    pub fn CVMetalTextureCacheFlushTextureCache(texture_cache: CVMetalTextureCacheRef, options: u64);
    pub fn CVMetalTextureGetTexture(image: CVMetalTextureRef) -> *mut c_void;

    pub fn CFRelease(cf: *mut c_void);
    pub fn CFNumberCreate(allocator: CFAllocatorRef, the_type: i64, value_ptr: *const c_void) -> CFNumberRef;
    pub fn CFStringCreateWithCString(allocator: CFAllocatorRef, c_str: *const c_char, encoding: u32) -> CFStringRef;
    pub fn CFDictionaryCreate(
        allocator: CFAllocatorRef,
        keys: *const *const c_void,
        values: *const *const c_void,
        num_values: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> CFMutableDictionaryRef;

    pub static kCFTypeDictionaryKeyCallBacks: c_void;
    pub static kCFTypeDictionaryValueCallBacks: c_void;
    pub static kCFBooleanTrue: CFBooleanRef;
    /// el allocator que NO copia ni libera: el bloque de bytes es nuestro
    pub static kCFAllocatorNull: CFAllocatorRef;
    pub static kCFBooleanFalse: CFBooleanRef;
    pub static kVTVideoEncoderSpecification_EnableHardwareAcceleratedVideoEncoder: CFStringRef;
    pub static kVTCompressionPropertyKey_RealTime: CFStringRef;
    pub static kVTCompressionPropertyKey_MaximizePowerEfficiency: CFStringRef;
    pub static kVTCompressionPropertyKey_AverageBitRate: CFNumberRef;
    pub static kVTCompressionPropertyKey_ProfileLevel: CFStringRef;
    pub static kVTCompressionPropertyKey_AllowFrameReordering: CFBooleanRef;
    pub static kVTEncodeFrameOptionKey_ForceKeyFrame: CFStringRef;
    pub static kVTCompressionPropertyKey_ColorPrimaries: CFStringRef;
    pub static kVTCompressionPropertyKey_YCbCrMatrix: CFStringRef;
    pub static kVTCompressionPropertyKey_TransferFunction: CFStringRef;
    pub static kCVPixelBufferIOSurfacePropertiesKey: CFStringRef;
    pub static kCVPixelBufferPixelFormatTypeKey: CFStringRef;
    pub static kCVPixelBufferWidthKey: CFStringRef;
    pub static kCVPixelBufferHeightKey: CFStringRef;
    pub static kCVPixelBufferMetalCompatibilityKey: CFStringRef;
    pub static kCVPixelBufferPoolMinimumBufferCountKey: CFStringRef;
    pub static kCVImageBufferColorPrimariesKey: CFStringRef;
    pub static kCVImageBufferYCbCrMatrixKey: CFStringRef;
    pub static kCVImageBufferTransferFunctionKey: CFStringRef;
}

pub fn cfstr(s: &str) -> CFStringRef {
    let c = std::ffi::CString::new(s).unwrap();
    unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), 0x08000100) }
}

pub fn cfdict(pairs: &[(CFStringRef, *const c_void)]) -> CFMutableDictionaryRef {
    let keys: Vec<*const c_void> = pairs.iter().map(|(k, _)| *k as *const c_void).collect();
    let vals: Vec<*const c_void> = pairs.iter().map(|(_, v)| *v).collect();
    unsafe {
        CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            vals.as_ptr(),
            pairs.len() as isize,
            &kCFTypeDictionaryKeyCallBacks as *const _ as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const _ as *const c_void,
        )
    }
}
