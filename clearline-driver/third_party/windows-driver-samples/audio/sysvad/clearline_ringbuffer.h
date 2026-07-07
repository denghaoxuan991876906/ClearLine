/*++

ClearLine virtual microphone ring buffer contract.

--*/

#ifndef _CLEARLINE_RINGBUFFER_H_
#define _CLEARLINE_RINGBUFFER_H_

#define CLEARLINE_IOCTL_PING_INDEX              0x801
#define CLEARLINE_IOCTL_WRITE_PCM_INDEX         0x802
#define CLEARLINE_IOCTL_GET_BUFFER_STATUS_INDEX 0x803
#define IOCTL_CLEARLINE_PING                    CTL_CODE(FILE_DEVICE_UNKNOWN, CLEARLINE_IOCTL_PING_INDEX, METHOD_BUFFERED, FILE_ANY_ACCESS)
#define IOCTL_CLEARLINE_WRITE_PCM               CTL_CODE(FILE_DEVICE_UNKNOWN, CLEARLINE_IOCTL_WRITE_PCM_INDEX, METHOD_BUFFERED, FILE_ANY_ACCESS)
#define IOCTL_CLEARLINE_GET_BUFFER_STATUS       CTL_CODE(FILE_DEVICE_UNKNOWN, CLEARLINE_IOCTL_GET_BUFFER_STATUS_INDEX, METHOD_BUFFERED, FILE_ANY_ACCESS)

#define CLEARLINE_PING_MAGIC                    0x436C7243
#define CLEARLINE_PING_VERSION                  1
#define CLEARLINE_PCM_RING_BUFFER_BYTES         (48000 * sizeof(SHORT) * 2)
#define CLEARLINE_MAX_PCM_WRITE_BYTES           CLEARLINE_PCM_RING_BUFFER_BYTES
#define CLEARLINE_POOL_TAG                      'LrLC'

typedef struct _ClearLinePingResponse
{
    ULONG Magic;
    ULONG Version;
    ULONG SampleRateHz;
    ULONG Channels;
} ClearLinePingResponse;

typedef struct _ClearLineBufferStatus
{
    ULONG CapacityBytes;
    ULONG ReadableBytes;
    ULONG WritableBytes;
    ULONGLONG TotalWrittenBytes;
    ULONGLONG TotalDroppedBytes;
    ULONGLONG OverflowCount;
    ULONGLONG TotalReadBytes;
    ULONGLONG TotalUnderrunBytes;
    ULONGLONG UnderrunCount;
} ClearLineBufferStatus;

NTSTATUS
ClearLineInitializeRingBuffer();

void
ClearLineDestroyRingBuffer();

NTSTATUS
ClearLineWritePcmToRingBuffer(
    _In_reads_bytes_(BytesToWrite) PUCHAR Source,
    _In_ ULONG BytesToWrite,
    _Out_opt_ ULONG* BytesAccepted
);

NTSTATUS
ClearLineReadPcmFromRingBuffer(
    _Out_writes_bytes_(BytesToRead) PUCHAR Destination,
    _In_ ULONG BytesToRead,
    _Out_opt_ ULONG* BytesRead
);

void
ClearLineFillCaptureBuffer(
    _Out_writes_bytes_(BytesToFill) PUCHAR Destination,
    _In_ ULONG BytesToFill
);

ULONG
ClearLineGetRingBufferReadableBytes();

void
ClearLineFillBufferStatus(
    _Out_ ClearLineBufferStatus* Status
);

#endif // _CLEARLINE_RINGBUFFER_H_
